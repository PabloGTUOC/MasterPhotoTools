//! The server's half of the handoff: what to send, and what actually arrived.
//!
//! Both halves of F16 live here. [`decide`] answers the manifest *before* any
//! photograph is copied, so known duplicates never cross the network at all;
//! [`verify_arrivals`] checks what did cross, because *"an interrupted copy
//! leaves a file whose checksum fails to match the manifest; it is simply
//! recopied"* (specification §2.3).

use super::manifest::Manifest;
use crate::error::Error;
use crate::ingest::scanner::hash_file;
use crate::ledger::Ledger;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// What the server wants done with one manifest entry.
///
/// **Three answers, not two.** The build plan describes the reply as "which
/// are new", but a plain new/known split has to decide what "known" means, and
/// both readings are wrong:
///
/// - If "known" means *published*, then a photograph whose bytes are already on
///   the NAS gets copied a second time for nothing.
/// - If "known" means *seen*, then a photograph that was staged but whose
///   publish failed is never retried. It is silently lost — which is the exact
///   failure F16 exists to prevent, arriving from the other direction.
///
/// Separating "do not send the bytes" from "do not publish it" answers both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Disposition {
    /// Not seen before. Send the bytes.
    Send,
    /// The bytes are already in the staging directory, but this photograph has
    /// never been published. Do not transfer it again; do publish it.
    AlreadyStaged,
    /// Published before. Nothing to send, nothing to publish (F16).
    AlreadyPublished,
}

impl Disposition {
    /// Whether the desktop must copy this file.
    pub fn needs_transfer(self) -> bool {
        matches!(self, Disposition::Send)
    }

    /// Whether the file must be present and intact in staging before the server
    /// can go on.
    pub fn expects_a_file(self) -> bool {
        !matches!(self, Disposition::AlreadyPublished)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Disposition::Send => "send",
            Disposition::AlreadyStaged => "already_staged",
            Disposition::AlreadyPublished => "already_published",
        }
    }
}

/// One entry's answer. Ordered to match the manifest, and self-describing so a
/// client never has to trust that alignment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryPlan {
    pub stem: String,
    pub source_sha256: String,
    pub file_name: String,
    pub disposition: Disposition,
}

/// The reply to a manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionPlan {
    pub session_id: String,
    pub entries: Vec<EntryPlan>,
}

impl SessionPlan {
    pub fn to_send(&self) -> impl Iterator<Item = &EntryPlan> {
        self.entries
            .iter()
            .filter(|e| e.disposition.needs_transfer())
    }

    pub fn count(&self, disposition: Disposition) -> usize {
        self.entries
            .iter()
            .filter(|e| e.disposition == disposition)
            .count()
    }

    /// How many bytes this plan will actually put on the network.
    pub fn bytes_to_transfer(&self, manifest: &Manifest) -> u64 {
        self.to_send()
            .filter_map(|e| manifest.entry(&e.file_name))
            .map(|e| e.bytes)
            .sum()
    }
}

/// Answer a manifest: for each entry, send, already staged, or already
/// published (F16).
///
/// The published check is one query for the whole manifest rather than one per
/// entry, because a 400-frame card would otherwise make 400 round trips through
/// SQLite to learn something a single `IN` clause answers.
///
/// The staged check is deliberately **existence and size, not hash**. Hashing
/// what is already on disk would read gigabytes to answer a question that
/// [`verify_arrivals`] is about to ask properly anyway; a file that is present,
/// the right length and nevertheless corrupt is caught there and recopied,
/// which is the path that already exists for exactly this case.
pub fn decide(
    manifest: &Manifest,
    ledger: &Ledger,
    staging_dir: &Path,
) -> Result<SessionPlan, Error> {
    manifest.validate()?;

    let source_hashes: Vec<String> = manifest
        .entries
        .iter()
        .map(|e| e.source_sha256.clone())
        .collect();
    let published = ledger
        .published_among(&source_hashes)
        .map_err(|e| Error::Internal(e.to_string()))?;

    let entries = manifest
        .entries
        .iter()
        .map(|entry| {
            let disposition = if published.contains(&entry.source_sha256) {
                Disposition::AlreadyPublished
            } else if already_staged(staging_dir, &entry.file_name, entry.bytes) {
                Disposition::AlreadyStaged
            } else {
                Disposition::Send
            };

            EntryPlan {
                stem: entry.stem.clone(),
                source_sha256: entry.source_sha256.clone(),
                file_name: entry.file_name.clone(),
                disposition,
            }
        })
        .collect();

    Ok(SessionPlan {
        session_id: manifest.session_id.clone(),
        entries,
    })
}

fn already_staged(staging_dir: &Path, file_name: &str, expected_bytes: u64) -> bool {
    std::fs::metadata(staging_dir.join(file_name))
        .map(|m| m.is_file() && m.len() == expected_bytes)
        .unwrap_or(false)
}

/// Why a staged file has to be sent again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecopyReason {
    /// Nothing arrived under that name.
    Missing,
    /// Something arrived, but not all of it — the interrupted-copy case.
    ShortFile,
    /// The right length and the wrong contents.
    HashMismatch,
}

impl RecopyReason {
    pub fn as_str(self) -> &'static str {
        match self {
            RecopyReason::Missing => "missing",
            RecopyReason::ShortFile => "short_file",
            RecopyReason::HashMismatch => "hash_mismatch",
        }
    }
}

/// One file the desktop must send again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recopy {
    pub stem: String,
    pub file_name: String,
    pub reason: RecopyReason,
}

/// What survived the copy.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArrivalReport {
    pub session_id: String,
    /// Staged names that hashed to what the manifest promised.
    pub verified: Vec<String>,
    /// Files to send again. **Not failures** — §2.3 says an interrupted copy is
    /// simply recopied, so this is a request, not a verdict.
    pub recopy: Vec<Recopy>,
}

impl ArrivalReport {
    pub fn complete(&self) -> bool {
        self.recopy.is_empty()
    }
}

/// Hash every file the plan expected and report what has to be sent again.
///
/// Entries the plan settled as [`Disposition::AlreadyPublished`] are skipped:
/// their bytes were never requested, so demanding them on disk would fail a
/// session for a photograph that is already safely in Google Photos.
pub fn verify_arrivals(
    manifest: &Manifest,
    plan: &SessionPlan,
    staging_dir: &Path,
    progress: &dyn crate::jobs::Progress,
) -> ArrivalReport {
    let expected: Vec<&EntryPlan> = plan
        .entries
        .iter()
        .filter(|e| e.disposition.expects_a_file())
        .collect();

    let total = expected.len() as u64;
    let mut report = ArrivalReport {
        session_id: manifest.session_id.clone(),
        ..Default::default()
    };

    for (i, planned) in expected.iter().enumerate() {
        progress.report(i as u64, total, "verifying staged files");

        let Some(entry) = manifest.entry(&planned.file_name) else {
            // A plan naming something the manifest does not is a protocol
            // fault, not a transfer fault. Ask for it again rather than
            // pretending it verified.
            report.recopy.push(Recopy {
                stem: planned.stem.clone(),
                file_name: planned.file_name.clone(),
                reason: RecopyReason::Missing,
            });
            continue;
        };

        let path = staging_dir.join(&entry.file_name);
        let reason = match std::fs::metadata(&path) {
            Err(_) => Some(RecopyReason::Missing),
            Ok(m) if m.len() < entry.bytes => Some(RecopyReason::ShortFile),
            Ok(m) if m.len() != entry.bytes => Some(RecopyReason::HashMismatch),
            Ok(_) => match hash_file(&path) {
                Ok(actual) if actual == entry.derived_sha256 => None,
                // An unreadable file is a file to send again, not a crash.
                Ok(_) | Err(_) => Some(RecopyReason::HashMismatch),
            },
        };

        match reason {
            None => report.verified.push(entry.file_name.clone()),
            Some(reason) => report.recopy.push(Recopy {
                stem: entry.stem.clone(),
                file_name: entry.file_name.clone(),
                reason,
            }),
        }
    }

    progress.report(total, total, "verifying staged files");
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest::handoff::manifest::{Handoff, HandoffItem};
    use crate::jobs::{InMemoryProgress, Progress};
    use std::path::PathBuf;
    use std::sync::Mutex;

    /// A [`Progress`] that remembers what it was told.
    ///
    /// `InMemoryProgress` discards its reports, which is right for the tests
    /// that only need a sink — but it cannot answer whether progress was
    /// actually reported.
    #[derive(Default)]
    struct RecordingProgress {
        seen: Mutex<Vec<(u64, u64)>>,
    }

    impl RecordingProgress {
        fn latest(&self) -> (u64, u64) {
            *self.seen.lock().unwrap().last().unwrap()
        }
    }

    impl Progress for RecordingProgress {
        fn report(&self, done: u64, total: u64, _message: &str) {
            self.seen.lock().unwrap().push((done, total));
        }

        fn cancelled(&self) -> bool {
            false
        }
    }

    struct Fixture {
        _temp: tempfile::TempDir,
        source: PathBuf,
        staging: PathBuf,
        handoff: Handoff,
        ledger: Ledger,
    }

    fn fixture(files: &[(&str, &[u8])]) -> Fixture {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("derived");
        let staging = temp.path().join("staging");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&staging).unwrap();

        let items: Vec<HandoffItem> = files
            .iter()
            .map(|(stem, bytes)| {
                let path = source.join(format!("{stem}.jpg"));
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

        Fixture {
            handoff: Handoff::prepare("session-1", "card-1", &items).unwrap(),
            ledger: Ledger::open_in_memory().unwrap(),
            _temp: temp,
            source,
            staging,
        }
    }

    /// Put a file into staging exactly as the transfer step would.
    fn place(f: &Fixture, file_name: &str) {
        let local = f.handoff.local_path(file_name).unwrap();
        std::fs::copy(local, f.staging.join(file_name)).unwrap();
    }

    #[test]
    fn an_unknown_card_is_all_send() {
        let f = fixture(&[("IMG_0001", b"one"), ("IMG_0002", b"two")]);

        let plan = decide(f.handoff.manifest(), &f.ledger, &f.staging).unwrap();

        assert_eq!(plan.count(Disposition::Send), 2);
        assert_eq!(plan.bytes_to_transfer(f.handoff.manifest()), 6);
    }

    #[test]
    fn a_published_photograph_is_neither_sent_nor_published_again() {
        // F16: re-ingesting a card that has already been processed must publish
        // nothing.
        let f = fixture(&[("IMG_0001", b"one"), ("IMG_0002", b"two")]);
        f.ledger
            .record_published("source-of-IMG_0001", "IMG_0001", "d1", "session-0", None)
            .unwrap();

        let plan = decide(f.handoff.manifest(), &f.ledger, &f.staging).unwrap();

        assert_eq!(plan.entries[0].disposition, Disposition::AlreadyPublished);
        assert_eq!(plan.entries[1].disposition, Disposition::Send);
        assert_eq!(plan.bytes_to_transfer(f.handoff.manifest()), 3);
    }

    #[test]
    fn a_staged_but_unpublished_photograph_is_not_resent_and_not_forgotten() {
        // The interrupted-publish case. Its bytes are already on the NAS, so
        // there is nothing to transfer — but it has never reached Google
        // Photos, so it must still be published. A plain new/known reply cannot
        // express this, and whichever way it answered would be wrong.
        let f = fixture(&[("IMG_0001", b"one")]);
        let name = f.handoff.manifest().entries[0].file_name.clone();
        place(&f, &name);

        let plan = decide(f.handoff.manifest(), &f.ledger, &f.staging).unwrap();

        assert_eq!(plan.entries[0].disposition, Disposition::AlreadyStaged);
        assert!(!plan.entries[0].disposition.needs_transfer());
        assert!(plan.entries[0].disposition.expects_a_file());
        assert_eq!(plan.bytes_to_transfer(f.handoff.manifest()), 0);
    }

    #[test]
    fn a_staged_file_of_the_wrong_length_is_sent_again() {
        let f = fixture(&[("IMG_0001", b"one")]);
        let name = f.handoff.manifest().entries[0].file_name.clone();
        std::fs::write(f.staging.join(&name), b"o").unwrap();

        let plan = decide(f.handoff.manifest(), &f.ledger, &f.staging).unwrap();

        assert_eq!(plan.entries[0].disposition, Disposition::Send);
    }

    #[test]
    fn a_card_larger_than_sqlites_parameter_limit_still_deduplicates() {
        // SQLite's default limit is 999 bound parameters. A 400-frame card is
        // under it; two cards in one session, or a camera that writes bursts,
        // are not. The query is chunked, and this is what proves it.
        let ledger = Ledger::open_in_memory().unwrap();
        let hashes: Vec<String> = (0..1500).map(|i| format!("hash-{i:04}")).collect();
        for hash in hashes.iter().step_by(2) {
            ledger
                .record_published(hash, "stem", "d", "s", None)
                .unwrap();
        }

        let found = ledger.published_among(&hashes).unwrap();

        assert_eq!(found.len(), 750);
        assert!(found.contains("hash-0000"));
        assert!(!found.contains("hash-0001"));
    }

    #[test]
    fn an_empty_manifest_is_answered_rather_than_refused() {
        let f = fixture(&[]);
        let plan = decide(f.handoff.manifest(), &f.ledger, &f.staging).unwrap();
        assert!(plan.entries.is_empty());
    }

    #[test]
    fn a_hostile_file_name_is_refused_before_anything_is_read() {
        let f = fixture(&[("IMG_0001", b"one")]);
        let mut manifest = f.handoff.manifest().clone();
        manifest.entries[0].file_name = "../../escape.jpg".into();

        let err = decide(&manifest, &f.ledger, &f.staging).unwrap_err();
        assert!(matches!(err, Error::AccessDenied(_)), "got {err}");
    }

    #[test]
    fn files_that_arrived_intact_verify() {
        let f = fixture(&[("IMG_0001", b"one"), ("IMG_0002", b"two")]);
        let plan = decide(f.handoff.manifest(), &f.ledger, &f.staging).unwrap();
        for entry in &f.handoff.manifest().entries {
            place(&f, &entry.file_name);
        }

        let report = verify_arrivals(
            f.handoff.manifest(),
            &plan,
            &f.staging,
            &InMemoryProgress::new(),
        );

        assert!(report.complete());
        assert_eq!(report.verified.len(), 2);
    }

    #[test]
    fn a_truncated_staged_file_is_detected_and_asked_for_again() {
        // Build plan acceptance for this phase, and §2.3's stated behaviour.
        let f = fixture(&[("IMG_0001", b"a whole photograph")]);
        let plan = decide(f.handoff.manifest(), &f.ledger, &f.staging).unwrap();
        let name = f.handoff.manifest().entries[0].file_name.clone();
        std::fs::write(f.staging.join(&name), b"a whole").unwrap();

        let report = verify_arrivals(
            f.handoff.manifest(),
            &plan,
            &f.staging,
            &InMemoryProgress::new(),
        );

        assert!(!report.complete());
        assert_eq!(report.recopy.len(), 1);
        assert_eq!(report.recopy[0].reason, RecopyReason::ShortFile);
        assert_eq!(report.recopy[0].stem, "IMG_0001");
        assert!(report.verified.is_empty());
    }

    #[test]
    fn a_file_of_the_right_length_and_wrong_contents_is_detected() {
        // A short read is the common failure; a bad sector is the quiet one.
        // Length alone would pass this, which is why the hash is checked.
        let f = fixture(&[("IMG_0001", b"the original")]);
        let plan = decide(f.handoff.manifest(), &f.ledger, &f.staging).unwrap();
        let name = f.handoff.manifest().entries[0].file_name.clone();
        std::fs::write(f.staging.join(&name), b"the corrupt!").unwrap();

        let report = verify_arrivals(
            f.handoff.manifest(),
            &plan,
            &f.staging,
            &InMemoryProgress::new(),
        );

        assert_eq!(report.recopy[0].reason, RecopyReason::HashMismatch);
    }

    #[test]
    fn a_file_that_never_arrived_is_reported_as_missing() {
        let f = fixture(&[("IMG_0001", b"one")]);
        let plan = decide(f.handoff.manifest(), &f.ledger, &f.staging).unwrap();

        let report = verify_arrivals(
            f.handoff.manifest(),
            &plan,
            &f.staging,
            &InMemoryProgress::new(),
        );

        assert_eq!(report.recopy[0].reason, RecopyReason::Missing);
    }

    #[test]
    fn an_already_published_entry_is_not_expected_on_disk() {
        // Its bytes were never asked for. Demanding them would fail a session
        // over a photograph that is already safely published.
        let f = fixture(&[("IMG_0001", b"one")]);
        f.ledger
            .record_published("source-of-IMG_0001", "IMG_0001", "d1", "session-0", None)
            .unwrap();
        let plan = decide(f.handoff.manifest(), &f.ledger, &f.staging).unwrap();

        let report = verify_arrivals(
            f.handoff.manifest(),
            &plan,
            &f.staging,
            &InMemoryProgress::new(),
        );

        assert!(report.complete(), "got {:?}", report.recopy);
        assert!(report.verified.is_empty());
    }

    #[test]
    fn verification_reports_progress() {
        let f = fixture(&[("IMG_0001", b"one"), ("IMG_0002", b"two")]);
        let plan = decide(f.handoff.manifest(), &f.ledger, &f.staging).unwrap();
        for entry in &f.handoff.manifest().entries {
            place(&f, &entry.file_name);
        }

        let progress = RecordingProgress::default();
        verify_arrivals(f.handoff.manifest(), &plan, &f.staging, &progress);

        assert_eq!(progress.latest(), (2, 2));
    }

    #[test]
    fn the_source_directory_is_untouched_by_verification() {
        let f = fixture(&[("IMG_0001", b"one")]);
        let plan = decide(f.handoff.manifest(), &f.ledger, &f.staging).unwrap();
        place(&f, &f.handoff.manifest().entries[0].file_name);

        verify_arrivals(
            f.handoff.manifest(),
            &plan,
            &f.staging,
            &InMemoryProgress::new(),
        );

        assert_eq!(std::fs::read_dir(&f.source).unwrap().count(), 1);
    }
}
