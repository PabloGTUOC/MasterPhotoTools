//! The desktop-to-server handoff (F16).
//!
//! ```text
//!   desktop                                server
//!     │  manifest: hashes, no bytes  ──────▶ │  decide: send / staged / published
//!     │  ◀──────────────────────── plan      │
//!     │  copy only what was asked for        │
//!     │  ready ────────────────────────────▶ │  verify: hash every arrival
//!     │  ◀───────────────────── report       │
//!     │  recopy whatever failed ───────────▶ │  (until clean, or give up loudly)
//! ```
//!
//! **Why a staging directory rather than an upload API** (specification §2.3):
//! the Mac already mounts the NAS share over SMB, so writing into a watched
//! directory needs no upload protocol, no chunking and no resume logic. An
//! interrupted copy leaves a file whose checksum fails to match the manifest,
//! and it is simply recopied.
//!
//! The whole protocol lives here rather than in either binary, because both
//! halves of it are policy: which files to copy, what a mismatch means, and how
//! many times to try again. The binaries supply only the wire (G1) —
//! [`SessionClient`] is the seam, and it is what lets the entire exchange,
//! recopy rounds and all, be tested without an HTTP server.

pub mod manifest;
pub mod session;

pub use manifest::{items_for, Handoff, HandoffItem, Manifest, ManifestEntry, NotReady};
pub use session::{
    decide, verify_arrivals, ArrivalReport, Disposition, EntryPlan, Recopy, RecopyReason,
    SessionPlan,
};

use crate::error::Error;
use crate::jobs::Progress;
use std::path::Path;

/// How many times a file may be sent again before the session gives up.
///
/// A bounded loop rather than a patient one. A copy that fails three times is
/// not a transient network hiccup — it is a failing disk, a full volume or a
/// share that went away, and none of those get better by trying a fourth time.
/// The session stops and says which photographs did not make it, which is
/// something a person can act on.
pub const MAX_RECOPY_ROUNDS: u32 = 3;

/// The transport the desktop supplies. Two calls, both from specification §8.
///
/// Synchronous on purpose: the handoff runs inside a job on its own thread
/// (F17), so it may block, and an async trait here would push `tokio` into
/// `core` for no gain.
pub trait SessionClient {
    /// `POST /api/ingest/sessions` — manifest in, dispositions out.
    fn open_session(&self, manifest: &Manifest) -> Result<SessionPlan, Error>;

    /// `POST /api/ingest/sessions/{id}/ready` — the staged files are written;
    /// verify them and say what has to come again.
    fn mark_ready(&self, session_id: &str) -> Result<ArrivalReport, Error>;
}

/// What one handoff did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HandoffOutcome {
    pub session_id: String,
    /// Files copied, counting a recopy each time it happened.
    pub copied: usize,
    /// Bytes actually put on the network.
    pub bytes_transferred: u64,
    pub already_staged: usize,
    pub already_published: usize,
    /// How many files had to be sent a second time or later.
    pub recopied: usize,
    /// How many `ready` rounds it took. One means nothing needed recopying.
    pub rounds: u32,
    /// Files that never arrived intact. Empty on success.
    pub unresolved: Vec<Recopy>,
}

impl HandoffOutcome {
    pub fn complete(&self) -> bool {
        self.unresolved.is_empty()
    }

    /// A line for a job's completion message.
    pub fn describe(&self) -> String {
        let mut parts = vec![format!(
            "{} transferred ({})",
            self.copied,
            human_bytes(self.bytes_transferred)
        )];
        if self.already_published > 0 {
            parts.push(format!("{} already published", self.already_published));
        }
        if self.already_staged > 0 {
            parts.push(format!("{} already staged", self.already_staged));
        }
        if self.recopied > 0 {
            parts.push(format!("{} recopied", self.recopied));
        }
        if !self.unresolved.is_empty() {
            parts.push(format!("{} could not be copied", self.unresolved.len()));
        }
        parts.join(", ")
    }
}

fn human_bytes(bytes: u64) -> String {
    const MB: u64 = 1024 * 1024;
    match bytes {
        0 => "no bytes".into(),
        b if b < 1024 => format!("{b} B"),
        b if b < MB => format!("{:.1} KB", b as f64 / 1024.0),
        b => format!("{:.1} MB", b as f64 / MB as f64),
    }
}

/// Run one card's handoff to completion.
///
/// The order is F16's, and it is the reason this function exists at all: the
/// manifest goes first and the bytes go second, so a card that has already been
/// ingested costs one small request and no transfer whatsoever.
pub fn run_handoff(
    handoff: &Handoff,
    staging_dir: &Path,
    client: &dyn SessionClient,
    progress: &dyn Progress,
) -> Result<HandoffOutcome, Error> {
    let manifest = handoff.manifest();
    manifest.validate()?;

    let plan = client.open_session(manifest)?;

    let mut outcome = HandoffOutcome {
        session_id: plan.session_id.clone(),
        already_staged: plan.count(Disposition::AlreadyStaged),
        already_published: plan.count(Disposition::AlreadyPublished),
        ..Default::default()
    };

    let to_send: Vec<String> = plan.to_send().map(|e| e.file_name.clone()).collect();
    copy_into_staging(handoff, &to_send, staging_dir, progress, &mut outcome)?;

    for round in 1..=MAX_RECOPY_ROUNDS {
        outcome.rounds = round;

        let report = client.mark_ready(&plan.session_id)?;
        if report.complete() {
            outcome.unresolved.clear();
            return Ok(outcome);
        }

        // §2.3: a file whose checksum fails to match "is simply recopied".
        let again: Vec<String> = report.recopy.iter().map(|r| r.file_name.clone()).collect();
        outcome.unresolved = report.recopy;

        // Nothing below will succeed if the person asked for it to stop, and
        // asking the server again would only add load to a cancelled job.
        if round == MAX_RECOPY_ROUNDS || progress.cancelled() {
            break;
        }

        outcome.recopied += again.len();
        copy_into_staging(handoff, &again, staging_dir, progress, &mut outcome)?;
    }

    Ok(outcome)
}

/// Copy the named files into the staging directory.
///
/// Through a temporary name and a rename, exactly as
/// [`crate::ingest::staging`] does, and for the same reason: a copy interrupted
/// half way must not leave a file that looks complete and correctly named. The
/// server polls this directory, so a partially written file under its final
/// name would be read as an arrival and fail verification for no reason.
fn copy_into_staging(
    handoff: &Handoff,
    names: &[String],
    staging_dir: &Path,
    progress: &dyn Progress,
    outcome: &mut HandoffOutcome,
) -> Result<(), Error> {
    if names.is_empty() {
        return Ok(());
    }
    std::fs::create_dir_all(staging_dir)?;

    let total = names.len() as u64;
    for (i, name) in names.iter().enumerate() {
        if progress.cancelled() {
            break;
        }

        let source = handoff.local_path(name).ok_or_else(|| {
            Error::Internal(format!(
                "the server asked for {name}, which is not in this handoff"
            ))
        })?;

        let destination = staging_dir.join(name);
        let partial = staging_dir.join(format!("{name}.partial"));

        let bytes = std::fs::copy(source, &partial)?;
        std::fs::rename(&partial, &destination)?;

        outcome.copied += 1;
        outcome.bytes_transferred += bytes;
        progress.report(i as u64 + 1, total, "copying to the staging directory");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::InMemoryProgress;
    use std::cell::RefCell;
    use std::path::PathBuf;

    /// A server that answers from a real ledger and a real staging directory,
    /// so the protocol is tested against the same logic the HTTP handlers run.
    struct FakeServer {
        ledger: crate::ledger::Ledger,
        staging: PathBuf,
        manifest: RefCell<Option<Manifest>>,
        plan: RefCell<Option<SessionPlan>>,
        opens: RefCell<u32>,
        readies: RefCell<u32>,
        /// One file to quietly damage per `ready` call, standing in for a copy
        /// that was interrupted. Pushing the same name twice damages it on two
        /// consecutive rounds.
        damage_queue: RefCell<std::collections::VecDeque<String>>,
    }

    impl FakeServer {
        fn new(staging: PathBuf) -> Self {
            Self {
                ledger: crate::ledger::Ledger::open_in_memory().unwrap(),
                staging,
                manifest: RefCell::new(None),
                plan: RefCell::new(None),
                opens: RefCell::new(0),
                readies: RefCell::new(0),
                damage_queue: RefCell::new(std::collections::VecDeque::new()),
            }
        }
    }

    impl SessionClient for FakeServer {
        fn open_session(&self, manifest: &Manifest) -> Result<SessionPlan, Error> {
            *self.opens.borrow_mut() += 1;
            let plan = decide(manifest, &self.ledger, &self.staging)?;
            *self.manifest.borrow_mut() = Some(manifest.clone());
            *self.plan.borrow_mut() = Some(plan.clone());
            Ok(plan)
        }

        fn mark_ready(&self, _session_id: &str) -> Result<ArrivalReport, Error> {
            *self.readies.borrow_mut() += 1;

            if let Some(name) = self.damage_queue.borrow_mut().pop_front() {
                std::fs::write(self.staging.join(&name), b"truncated").unwrap();
            }

            let manifest = self.manifest.borrow();
            let plan = self.plan.borrow();
            Ok(verify_arrivals(
                manifest.as_ref().unwrap(),
                plan.as_ref().unwrap(),
                &self.staging,
                &InMemoryProgress::new(),
            ))
        }
    }

    struct Setup {
        _temp: tempfile::TempDir,
        staging: PathBuf,
        handoff: Handoff,
        server: FakeServer,
    }

    fn setup(files: &[(&str, &[u8])]) -> Setup {
        let temp = tempfile::tempdir().unwrap();
        let derived = temp.path().join("derived");
        let staging = temp.path().join("staging");
        std::fs::create_dir_all(&derived).unwrap();
        std::fs::create_dir_all(&staging).unwrap();

        let items: Vec<HandoffItem> = files
            .iter()
            .map(|(stem, bytes)| {
                let path = derived.join(format!("{stem}.jpg"));
                std::fs::write(&path, bytes).unwrap();
                HandoffItem {
                    stem: (*stem).into(),
                    source_sha256: format!("source-of-{stem}"),
                    derived: path,
                    width: 3000,
                    height: 2000,
                    capture: None,
                }
            })
            .collect();

        Setup {
            handoff: Handoff::prepare("session-1", "card-1", &items).unwrap(),
            server: FakeServer::new(staging.clone()),
            staging,
            _temp: temp,
        }
    }

    fn run(s: &Setup) -> HandoffOutcome {
        run_handoff(&s.handoff, &s.staging, &s.server, &InMemoryProgress::new()).unwrap()
    }

    #[test]
    fn a_fresh_card_transfers_everything_once() {
        let s = setup(&[("IMG_0001", b"one"), ("IMG_0002", b"two")]);

        let outcome = run(&s);

        assert!(outcome.complete());
        assert_eq!(outcome.copied, 2);
        assert_eq!(outcome.bytes_transferred, 6);
        assert_eq!(outcome.rounds, 1);
        assert_eq!(*s.server.readies.borrow(), 1);
    }

    #[test]
    fn the_manifest_is_answered_before_any_byte_is_copied() {
        // F16's actual requirement, and the thing a test of the end state
        // cannot show: that the order was manifest first, bytes second.
        //
        // The staging directory is checked from inside `open_session`, at the
        // one moment where "before" and "after" are distinguishable.
        struct OrderCheck {
            staging: PathBuf,
            empty_when_asked: RefCell<Option<bool>>,
            inner: FakeServer,
        }

        impl SessionClient for OrderCheck {
            fn open_session(&self, manifest: &Manifest) -> Result<SessionPlan, Error> {
                let count = std::fs::read_dir(&self.staging)
                    .map(|d| d.count())
                    .unwrap_or(0);
                *self.empty_when_asked.borrow_mut() = Some(count == 0);
                self.inner.open_session(manifest)
            }

            fn mark_ready(&self, id: &str) -> Result<ArrivalReport, Error> {
                self.inner.mark_ready(id)
            }
        }

        let s = setup(&[("IMG_0001", b"one"), ("IMG_0002", b"two")]);
        let client = OrderCheck {
            staging: s.staging.clone(),
            empty_when_asked: RefCell::new(None),
            inner: FakeServer::new(s.staging.clone()),
        };

        let outcome =
            run_handoff(&s.handoff, &s.staging, &client, &InMemoryProgress::new()).unwrap();

        assert_eq!(
            *client.empty_when_asked.borrow(),
            Some(true),
            "the manifest must be sent before any photograph is copied"
        );
        assert_eq!(outcome.copied, 2);
    }

    #[test]
    fn ingesting_the_same_card_twice_transfers_zero_bytes_and_publishes_nothing() {
        // Build plan acceptance for this phase, and F16 itself.
        let s = setup(&[("IMG_0001", b"one"), ("IMG_0002", b"two")]);

        let first = run(&s);
        assert_eq!(first.bytes_transferred, 6);

        // Phase 12 is what will call this for real, when Google Photos accepts
        // the photograph. Recording it here is what "already processed" means.
        for entry in &s.handoff.manifest().entries {
            s.server
                .ledger
                .record_published(
                    &entry.source_sha256,
                    &entry.stem,
                    &entry.derived_sha256,
                    "session-1",
                    Some("media-item-id"),
                )
                .unwrap();
        }

        let second = run(&s);

        assert_eq!(second.bytes_transferred, 0, "not one byte a second time");
        assert_eq!(second.copied, 0);
        assert_eq!(second.already_published, 2);
        assert_eq!(
            second.already_staged, 0,
            "a published photograph is settled by the ledger, not by the disk"
        );
        assert!(second.complete());
    }

    #[test]
    fn a_card_staged_but_never_published_is_not_resent_and_not_dropped() {
        // The interrupted-publish case: the bytes are already on the NAS, so
        // there is nothing to transfer — but nothing reached Google Photos
        // either, so these photographs must still be publishable. Treating
        // "already staged" as "already done" would lose them silently.
        let s = setup(&[("IMG_0001", b"one"), ("IMG_0002", b"two")]);

        let first = run(&s);
        assert_eq!(first.already_staged, 0);

        let second = run(&s);

        assert_eq!(second.bytes_transferred, 0);
        assert_eq!(second.already_staged, 2);
        assert_eq!(second.already_published, 0);
    }

    #[test]
    fn a_truncated_staged_file_is_recopied_rather_than_failed() {
        // Build plan acceptance for this phase.
        let s = setup(&[("IMG_0001", b"a whole photograph"), ("IMG_0002", b"two")]);
        let damaged = s.handoff.manifest().entries[0].file_name.clone();
        s.server
            .damage_queue
            .borrow_mut()
            .push_back(damaged.clone());

        let outcome = run(&s);

        assert!(outcome.complete(), "unresolved: {:?}", outcome.unresolved);
        assert_eq!(outcome.recopied, 1);
        assert_eq!(outcome.rounds, 2, "one round to find it, one to fix it");
        assert_eq!(
            std::fs::read(s.staging.join(&damaged)).unwrap(),
            b"a whole photograph"
        );
    }

    #[test]
    fn a_file_that_will_not_copy_is_reported_rather_than_retried_forever() {
        // A failing disk does not get better on the fourth attempt. The session
        // stops and names the photographs that did not make it.
        let s = setup(&[("IMG_0001", b"one")]);
        let name = s.handoff.manifest().entries[0].file_name.clone();
        for _ in 0..MAX_RECOPY_ROUNDS {
            s.server.damage_queue.borrow_mut().push_back(name.clone());
        }

        let outcome = run(&s);

        assert!(!outcome.complete());
        assert_eq!(outcome.rounds, MAX_RECOPY_ROUNDS);
        assert_eq!(outcome.unresolved.len(), 1);
        assert_eq!(outcome.unresolved[0].stem, "IMG_0001");
    }

    #[test]
    fn no_partial_file_survives_a_finished_handoff() {
        let s = setup(&[("IMG_0001", b"one"), ("IMG_0002", b"two")]);

        run(&s);

        let partials = std::fs::read_dir(&s.staging)
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".partial")
            })
            .count();
        assert_eq!(partials, 0);
    }

    #[test]
    fn an_empty_card_is_a_complete_handoff_that_moves_nothing() {
        let s = setup(&[]);

        let outcome = run(&s);

        assert!(outcome.complete());
        assert_eq!(outcome.copied, 0);
        assert_eq!(outcome.bytes_transferred, 0);
    }

    #[test]
    fn a_server_naming_a_file_this_handoff_does_not_have_is_an_error() {
        // Not a panic, and not a silent skip: the desktop cannot invent bytes
        // it was never given, and pretending otherwise would report success for
        // a photograph that never left the Mac.
        struct Liar;
        impl SessionClient for Liar {
            fn open_session(&self, manifest: &Manifest) -> Result<SessionPlan, Error> {
                Ok(SessionPlan {
                    session_id: manifest.session_id.clone(),
                    entries: vec![EntryPlan {
                        stem: "GHOST".into(),
                        source_sha256: "x".repeat(64),
                        file_name: "not-in-this-handoff.jpg".into(),
                        disposition: Disposition::Send,
                    }],
                })
            }
            fn mark_ready(&self, _: &str) -> Result<ArrivalReport, Error> {
                unreachable!("the copy fails before ready is reached")
            }
        }

        let s = setup(&[("IMG_0001", b"one")]);
        let err = run_handoff(&s.handoff, &s.staging, &Liar, &InMemoryProgress::new())
            .expect_err("a plan naming an unknown file must not be copied over");

        assert!(err.to_string().contains("not in this handoff"), "got {err}");
    }

    #[test]
    fn a_cancelled_handoff_stops_copying() {
        let s = setup(&[("IMG_0001", b"one"), ("IMG_0002", b"two")]);
        let progress = InMemoryProgress::new();
        progress.cancel();

        let outcome = run_handoff(&s.handoff, &s.staging, &s.server, &progress).unwrap();

        assert_eq!(outcome.copied, 0);
        assert!(!outcome.complete(), "nothing arrived, so nothing verified");
    }

    #[test]
    fn the_summary_line_says_what_happened() {
        let outcome = HandoffOutcome {
            copied: 12,
            bytes_transferred: 3 * 1024 * 1024,
            already_published: 400,
            ..Default::default()
        };
        assert_eq!(
            outcome.describe(),
            "12 transferred (3.0 MB), 400 already published"
        );
    }

    #[test]
    fn a_handoff_that_moved_nothing_says_so_plainly() {
        let outcome = HandoffOutcome {
            already_published: 400,
            ..Default::default()
        };
        assert_eq!(
            outcome.describe(),
            "0 transferred (no bytes), 400 already published"
        );
    }
}
