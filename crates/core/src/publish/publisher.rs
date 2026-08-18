//! Publishing a session to Google Photos (F15, §6.3).
//!
//! The hard part of this is not the two API calls. It is that **`batchCreate`
//! is not idempotent and the API cannot delete** (§6.1), so every decision here
//! is shaped by one question: if this goes wrong, does somebody end up with two
//! copies of a photograph they have to remove by hand?
//!
//! That produces three rules.
//!
//! 1. **State is recorded before the call that changes it**, not after. A
//!    process that dies mid-`batchCreate` leaves a row saying a create was in
//!    flight, which is different from a row saying one was never attempted.
//! 2. **An answer that never arrived is never retried.** §9.2 invariant 6: an
//!    operation reports only what it has verified, and a lost response has
//!    verified nothing. Those shots are reported as unconfirmed for a person to
//!    check, because the alternative is silently making duplicates.
//! 3. **Uploads may be retried freely; creates may not.** An unused upload token
//!    costs nothing and Google discards it.

use super::api::{
    rate_limit_delay, ApiError, CreateResult, NewMediaItem, PhotosApi, Sleeper, MAX_BATCH,
    MAX_RATE_LIMIT_RETRIES,
};
use crate::error::Error;
use crate::ingest::handoff::{ArrivalReport, Manifest, SessionPlan};
use crate::jobs::Progress;
use crate::ledger::{Ledger, PublishRow};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A source of access tokens, as a seam.
///
/// The publisher does not care how a token is obtained, only that it can ask
/// for a fresh one when the held one is refused.
///
/// Deliberately not `Send + Sync`: the obvious implementation is
/// [`crate::publish::Connector`], which holds a `rusqlite::Connection`, and that
/// is not `Sync`. A publish runs inside one job on one thread, so requiring more
/// would buy nothing and force every caller through a second mutex.
pub trait AccessTokens {
    fn access_token(&self) -> Result<String, Error>;
    /// The held token was refused; discard it so the next call fetches another.
    fn invalidate(&self);
}

/// One photograph to publish.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishItem {
    /// Deterministic, so re-running a session's publish resumes its rows rather
    /// than writing a second set of them.
    pub shot_id: String,
    pub stem: String,
    /// F16's deduplication key. Written to the published ledger, and only after
    /// Google confirms.
    pub source_sha256: String,
    pub file_name: String,
    pub bytes: u64,
}

/// A photograph that will not be published, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Skipped {
    pub stem: String,
    pub reason: String,
}

/// Where a session's shots already stand (§6.3).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumeCounts {
    pub pending: usize,
    /// Uploaded, with a token held. Resumes at `batchCreate`, never re-uploads.
    pub uploaded: usize,
    pub created: usize,
    /// A create was sent and no answer came back. **Not retried.**
    pub unconfirmed: usize,
}

/// What a publish would do. §9.2 rule 3 makes producing one mandatory first.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishPlan {
    pub session_id: String,
    pub items: Vec<PublishItem>,
    pub skipped: Vec<Skipped>,
    pub total_bytes: u64,
    /// One upload per photograph.
    pub upload_requests: usize,
    /// `ceil(items / 50)` — §6.1's batch limit.
    pub batch_create_requests: usize,
    pub resuming: ResumeCounts,
}

impl PublishPlan {
    pub fn describe(&self) -> String {
        if self.items.is_empty() {
            return format!("nothing to publish: {} shot(s) skipped", self.skipped.len());
        }
        format!(
            "{} photograph(s), {} to upload and {} batchCreate call(s); {} skipped",
            self.items.len(),
            self.upload_requests,
            self.batch_create_requests,
            self.skipped.len()
        )
    }
}

/// What a publish did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishOutcome {
    pub session_id: String,
    pub created: usize,
    pub uploaded: usize,
    /// Already `created` before this run started.
    pub already_created: usize,
    /// Sent, no answer. Needs a person to look in Google Photos (§9.2/6).
    pub unconfirmed: Vec<Skipped>,
    pub failed: Vec<Skipped>,
    pub skipped: Vec<Skipped>,
    pub batch_create_calls: usize,
    pub upload_calls: usize,
    /// Set when the run stopped early rather than finishing.
    pub halted: Option<String>,
}

impl PublishOutcome {
    pub fn complete(&self) -> bool {
        self.halted.is_none() && self.failed.is_empty() && self.unconfirmed.is_empty()
    }

    pub fn describe(&self) -> String {
        let mut parts = vec![format!("{} published", self.created)];
        if self.already_created > 0 {
            parts.push(format!("{} already published", self.already_created));
        }
        if !self.unconfirmed.is_empty() {
            parts.push(format!(
                "{} unconfirmed — check Google Photos before republishing",
                self.unconfirmed.len()
            ));
        }
        if !self.failed.is_empty() {
            parts.push(format!("{} failed", self.failed.len()));
        }
        if !self.skipped.is_empty() {
            parts.push(format!("{} skipped", self.skipped.len()));
        }
        if let Some(reason) = &self.halted {
            parts.push(format!("stopped: {reason}"));
        }
        parts.join(", ")
    }
}

/// A shot's publish identity: its session and the photograph itself.
///
/// Composite and readable rather than hashed, because the first thing anybody
/// does with a stuck publish row is look at it in the database.
pub fn shot_id(session_id: &str, source_sha256: &str) -> String {
    format!("{session_id}:{source_sha256}")
}

/// Which of a session's shots can be published, and which cannot.
///
/// Publishable means: the manifest exchange did not settle it as already
/// published, **and** its file verified on arrival. An unverified file is a file
/// that is not known to be intact, and uploading one is exactly the kind of
/// unverified success §9.2 invariant 6 forbids.
pub fn publishable(
    manifest: &Manifest,
    plan: &SessionPlan,
    report: Option<&ArrivalReport>,
) -> (Vec<PublishItem>, Vec<Skipped>) {
    let mut items = Vec::new();
    let mut skipped = Vec::new();

    for planned in &plan.entries {
        if !planned.disposition.expects_a_file() {
            skipped.push(Skipped {
                stem: planned.stem.clone(),
                reason: "already published".into(),
            });
            continue;
        }

        let Some(entry) = manifest.entry(&planned.file_name) else {
            skipped.push(Skipped {
                stem: planned.stem.clone(),
                reason: "not in the session's manifest".into(),
            });
            continue;
        };

        match report {
            None => skipped.push(Skipped {
                stem: planned.stem.clone(),
                reason: "the staged files have not been verified yet".into(),
            }),
            Some(report) if !report.verified.contains(&planned.file_name) => {
                let why = report
                    .recopy
                    .iter()
                    .find(|r| r.file_name == planned.file_name)
                    .map(|r| r.reason.as_str())
                    .unwrap_or("not verified");
                skipped.push(Skipped {
                    stem: planned.stem.clone(),
                    reason: format!("the staged file is {why}"),
                });
            }
            Some(_) => items.push(PublishItem {
                shot_id: shot_id(&plan.session_id, &entry.source_sha256),
                stem: entry.stem.clone(),
                source_sha256: entry.source_sha256.clone(),
                file_name: entry.file_name.clone(),
                bytes: entry.bytes,
            }),
        }
    }

    (items, skipped)
}

/// How many `batchCreate` calls a number of items needs (§6.1: at most 50).
pub fn batch_count(items: usize) -> usize {
    items.div_ceil(MAX_BATCH)
}

/// Work out what a publish would do, recording nothing.
///
/// Separate from [`dry_run`], and the separation is the safeguard. A publish
/// needs this plan too, and if computing it also stamped the session as
/// dry-run then every publish would satisfy its own precondition on the way
/// past — §9.2 rule 3 would be a check that could never fail.
pub fn plan_publish(
    manifest: &Manifest,
    plan: &SessionPlan,
    report: Option<&ArrivalReport>,
    ledger: &Ledger,
) -> Result<PublishPlan, Error> {
    let (items, skipped) = publishable(manifest, plan, report);

    let existing = ledger
        .publishes_for_session(&plan.session_id)
        .map_err(|e| Error::Internal(e.to_string()))?;
    let resuming = counts_of(&existing);

    // Only shots not already created need an upload and a create.
    let created: std::collections::HashSet<&str> = existing
        .iter()
        .filter(|row| row.state == "created")
        .map(|row| row.shot_id.as_str())
        .collect();
    let outstanding = items
        .iter()
        .filter(|item| !created.contains(item.shot_id.as_str()))
        .count();

    Ok(PublishPlan {
        session_id: plan.session_id.clone(),
        total_bytes: items.iter().map(|i| i.bytes).sum(),
        upload_requests: outstanding,
        batch_create_requests: batch_count(outstanding),
        items,
        skipped,
        resuming,
    })
}

/// Work out what a publish would do, **and** record that somebody looked
/// (§9.2 rule 3).
///
/// **Touches nothing outside the database.** No upload, no create, no request
/// to Google at all — which is the entire point of a dry run for an API that
/// cannot delete what it has made.
pub fn dry_run(
    manifest: &Manifest,
    plan: &SessionPlan,
    report: Option<&ArrivalReport>,
    ledger: &Ledger,
) -> Result<PublishPlan, Error> {
    let planned = plan_publish(manifest, plan, report, ledger)?;
    ledger
        .record_dry_run(&plan.session_id)
        .map_err(|e| Error::Internal(e.to_string()))?;
    Ok(planned)
}

fn counts_of(rows: &[PublishRow]) -> ResumeCounts {
    let mut counts = ResumeCounts::default();
    for row in rows {
        match row.state.as_str() {
            "uploaded" => counts.uploaded += 1,
            "created" => counts.created += 1,
            "creating" => counts.unconfirmed += 1,
            _ => counts.pending += 1,
        }
    }
    counts
}

/// Everything the publisher needs that is not the plan.
pub struct Publisher<'a> {
    pub ledger: &'a Ledger,
    pub api: &'a dyn PhotosApi,
    pub tokens: &'a dyn AccessTokens,
    pub sleeper: &'a dyn Sleeper,
    pub staging_dir: PathBuf,
}

impl Publisher<'_> {
    /// Publish a session.
    ///
    /// Refuses without a recorded dry run (§9.2 rule 3). That check is against
    /// the **database**, not a flag in memory: the API cannot delete, so a
    /// safeguard that a restart forgets is not a safeguard.
    pub fn publish(
        &self,
        plan: &PublishPlan,
        progress: &dyn Progress,
    ) -> Result<PublishOutcome, Error> {
        let dry_run_at = self
            .ledger
            .dry_run_at(&plan.session_id)
            .map_err(|e| Error::Internal(e.to_string()))?;

        if dry_run_at.is_none() {
            return Err(Error::Config(format!(
                "session {} has had no dry run. Google Photos cannot delete what \
                 it has created, so a mistaken publish is cleaned up by hand — \
                 review a dry run first (specification §9.2 rule 3).",
                plan.session_id
            )));
        }

        let mut outcome = PublishOutcome {
            session_id: plan.session_id.clone(),
            skipped: plan.skipped.clone(),
            ..Default::default()
        };

        for item in &plan.items {
            self.ledger
                .queue_publish(
                    &item.shot_id,
                    &plan.session_id,
                    &item.stem,
                    &item.source_sha256,
                    &item.file_name,
                )
                .map_err(|e| Error::Internal(e.to_string()))?;
        }

        if let Err(halt) = self.upload_phase(plan, &mut outcome, progress) {
            outcome.halted = Some(halt);
            return Ok(outcome);
        }

        if let Err(halt) = self.create_phase(plan, &mut outcome, progress) {
            outcome.halted = Some(halt);
        }

        Ok(outcome)
    }

    /// Upload every shot that has not already been uploaded.
    ///
    /// A shot already in `uploaded` or `created` is left alone: re-uploading is
    /// harmless to Google but would replace a held token, and §6.3's whole
    /// instruction is to resume from the recorded state rather than start again.
    fn upload_phase(
        &self,
        plan: &PublishPlan,
        outcome: &mut PublishOutcome,
        progress: &dyn Progress,
    ) -> Result<(), String> {
        let total = plan.items.len() as u64;

        for (index, item) in plan.items.iter().enumerate() {
            if progress.cancelled() {
                return Err("cancelled".into());
            }
            progress.report(index as u64, total, "uploading to Google Photos");

            let row = self.row(&item.shot_id)?;
            match row.as_ref().map(|r| r.state.as_str()) {
                Some("created") => {
                    outcome.already_created += 1;
                    continue;
                }
                Some("uploaded") | Some("creating") => continue,
                _ => {}
            }

            let path = self.staging_dir.join(&item.file_name);
            match self.upload_with_retries(&path, &item.stem, outcome) {
                Ok(token) => {
                    self.ledger
                        .record_upload(&item.shot_id, &token)
                        .map_err(|e| e.to_string())?;
                    outcome.uploaded += 1;
                }
                Err(Halt(reason)) => return Err(reason),
                Err(Failed(detail)) => {
                    self.ledger
                        .record_publish_failure(&item.shot_id, "pending", &detail)
                        .map_err(|e| e.to_string())?;
                    outcome.failed.push(Skipped {
                        stem: item.stem.clone(),
                        reason: detail,
                    });
                }
            }
        }

        Ok(())
    }

    /// Turn held upload tokens into media items, fifty at a time (§6.1).
    fn create_phase(
        &self,
        plan: &PublishPlan,
        outcome: &mut PublishOutcome,
        progress: &dyn Progress,
    ) -> Result<(), String> {
        let rows = self
            .ledger
            .publishes_for_session(&plan.session_id)
            .map_err(|e| e.to_string())?;

        // A create that was sent and never answered is left exactly where it is.
        for row in rows.iter().filter(|r| r.state == "creating") {
            outcome.unconfirmed.push(Skipped {
                stem: row.stem.clone(),
                reason: "a create was sent and Google's answer never arrived; \
                         check Google Photos before publishing this again"
                    .into(),
            });
        }

        let ready: Vec<&PublishRow> = rows
            .iter()
            .filter(|r| r.state == "uploaded" && r.upload_token.is_some())
            .collect();

        let total = ready.len() as u64;
        let mut done = 0u64;

        for chunk in ready.chunks(MAX_BATCH) {
            if progress.cancelled() {
                return Err("cancelled".into());
            }
            progress.report(done, total, "creating media items");

            let items: Vec<NewMediaItem> = chunk
                .iter()
                .map(|row| NewMediaItem {
                    upload_token: row.upload_token.clone().unwrap_or_default(),
                    file_name: format!("{}.jpg", row.stem),
                })
                .collect();

            // Before the call, never after. A process that dies here must leave
            // evidence that a create was in flight.
            for row in chunk {
                self.ledger
                    .record_creating(&row.shot_id)
                    .map_err(|e| e.to_string())?;
            }

            match self.create_with_retries(&items, outcome) {
                Ok(results) => self.settle(chunk, &results, outcome)?,
                Err(Halt(reason)) => return Err(reason),
                Err(Failed(detail)) => {
                    // A definite failure — nothing was created. Back to
                    // `uploaded`, where the held token makes a retry free.
                    for row in chunk {
                        self.ledger
                            .record_publish_failure(&row.shot_id, "uploaded", &detail)
                            .map_err(|e| e.to_string())?;
                        outcome.failed.push(Skipped {
                            stem: row.stem.clone(),
                            reason: detail.clone(),
                        });
                    }
                }
            }

            done += chunk.len() as u64;
        }

        progress.report(total, total, "creating media items");
        Ok(())
    }

    /// Record what a `batchCreate` said, item by item.
    fn settle(
        &self,
        chunk: &[&PublishRow],
        results: &[CreateResult],
        outcome: &mut PublishOutcome,
    ) -> Result<(), String> {
        for (row, result) in chunk.iter().zip(results) {
            match result {
                CreateResult::Created { media_item_id } => {
                    // The media item and F16's ledger entry are written
                    // together. A crash between them would leave a photograph
                    // published but absent from the deduplication ledger, and
                    // the next ingest of that card would publish it again.
                    self.ledger
                        .record_created_and_published(
                            &row.shot_id,
                            media_item_id,
                            &row.source_sha256,
                            &row.stem,
                            &row.file_name,
                        )
                        .map_err(|e| e.to_string())?;
                    outcome.created += 1;
                }
                CreateResult::Failed { detail } => {
                    self.ledger
                        .record_publish_failure(&row.shot_id, "uploaded", detail)
                        .map_err(|e| e.to_string())?;
                    outcome.failed.push(Skipped {
                        stem: row.stem.clone(),
                        reason: detail.clone(),
                    });
                }
            }
        }
        Ok(())
    }

    fn upload_with_retries(
        &self,
        path: &Path,
        stem: &str,
        outcome: &mut PublishOutcome,
    ) -> Result<String, Attempt> {
        let mut rate_limits = 0u32;
        let mut refreshed = false;

        loop {
            let token = self.token()?;
            outcome.upload_calls += 1;

            match self.api.upload(&token, path, &format!("{stem}.jpg")) {
                Ok(upload_token) => return Ok(upload_token),
                Err(ApiError::RateLimited { retry_after }) => {
                    self.wait_out(&mut rate_limits, retry_after)?;
                }
                Err(ApiError::Unauthorized) if !refreshed => {
                    refreshed = true;
                    self.tokens.invalidate();
                }
                // An upload is safe to retry, but not endlessly: a second 401
                // after a fresh token is a permissions problem, not a stale one.
                Err(e) => return Err(Attempt::Failed(e.to_string())),
            }
        }
    }

    fn create_with_retries(
        &self,
        items: &[NewMediaItem],
        outcome: &mut PublishOutcome,
    ) -> Result<Vec<CreateResult>, Attempt> {
        let mut rate_limits = 0u32;
        let mut refreshed = false;

        loop {
            let token = self.token()?;
            outcome.batch_create_calls += 1;

            match self.api.batch_create(&token, items) {
                Ok(results) => return Ok(results),
                Err(ApiError::RateLimited { retry_after }) => {
                    // A 429 is Google declining to act. Nothing was created, so
                    // waiting and repeating the call is safe.
                    self.wait_out(&mut rate_limits, retry_after)?;
                }
                Err(ApiError::Unauthorized) if !refreshed => {
                    refreshed = true;
                    self.tokens.invalidate();
                }
                Err(e) if e.may_have_been_applied() => {
                    // The rows stay at `creating`, which is what makes them
                    // unconfirmed rather than retried.
                    return Err(Attempt::Halt(format!(
                        "{e}. The photographs in this batch may or may not have \
                         been created; they are left unconfirmed rather than \
                         sent again, because a second attempt would duplicate \
                         any that succeeded and Google Photos cannot delete."
                    )));
                }
                Err(e) => return Err(Attempt::Failed(e.to_string())),
            }
        }
    }

    /// Wait out a `429` (§6.1: at least thirty seconds, then exponential).
    fn wait_out(
        &self,
        seen: &mut u32,
        retry_after: Option<std::time::Duration>,
    ) -> Result<(), Attempt> {
        if *seen >= MAX_RATE_LIMIT_RETRIES {
            return Err(Attempt::Halt(format!(
                "Google rate-limited this account {MAX_RATE_LIMIT_RETRIES} times \
                 in a row. Stopping rather than hammering it; the session resumes \
                 from where it stopped."
            )));
        }
        self.sleeper.sleep(rate_limit_delay(*seen, retry_after));
        *seen += 1;
        Ok(())
    }

    /// An access token, or a reason to stop the whole run.
    ///
    /// A dead grant halts rather than failing each shot: without this, one
    /// expired refresh token turns a 400-photograph session into 400 identical
    /// errors, which is the loop the build plan says not to write.
    fn token(&self) -> Result<String, Attempt> {
        self.tokens.access_token().map_err(|e| {
            Attempt::Halt(format!(
                "{e} No further photographs were attempted; publishing resumes \
                 where it stopped once the account is reconnected."
            ))
        })
    }

    fn row(&self, shot_id: &str) -> Result<Option<PublishRow>, String> {
        self.ledger.publish_row(shot_id).map_err(|e| e.to_string())
    }
}

/// Why one attempt did not produce a result.
enum Attempt {
    /// This photograph failed; the rest of the run continues.
    Failed(String),
    /// Nothing further will work; stop the run and say so.
    Halt(String),
}

use Attempt::{Failed, Halt};

impl From<Attempt> for String {
    fn from(a: Attempt) -> String {
        match a {
            Failed(d) | Halt(d) => d,
        }
    }
}
