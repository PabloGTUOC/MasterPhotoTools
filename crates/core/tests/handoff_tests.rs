//! Phase 11 acceptance: the desktop-to-server handoff and the ledger (F16).
//!
//! These run the whole path a real card takes — scan, pair, choose what is
//! publishable, exchange a manifest, copy, verify — against fixture cards on
//! disk. The server is a fake only in that it is called directly rather than
//! over HTTP; the decisions it makes are `core`'s own, which is what the HTTP
//! handlers call too.

mod fixtures;

use fixtures::{Fixtures, ShotKind};
use phototools_core::error::Error;
use phototools_core::ingest::{
    self, ArrivalReport, Card, Handoff, Manifest, SessionClient, SessionPlan,
};
use phototools_core::jobs::InMemoryProgress;
use phototools_core::ledger::Ledger;
use std::cell::RefCell;
use std::path::{Path, PathBuf};

/// The server's half, called directly. Same `decide` and `verify_arrivals` the
/// HTTP handlers run, and a real SQLite ledger underneath.
struct Server {
    ledger: Ledger,
    staging: PathBuf,
    agreed: RefCell<Option<(Manifest, SessionPlan)>>,
}

impl Server {
    fn new(staging: PathBuf) -> Self {
        Self {
            ledger: Ledger::open_in_memory().unwrap(),
            staging,
            agreed: RefCell::new(None),
        }
    }

    /// What Phase 12 will do when Google Photos accepts a photograph.
    fn publish_everything(&self, manifest: &Manifest) {
        for entry in &manifest.entries {
            self.ledger
                .record_published(
                    &entry.source_sha256,
                    &entry.stem,
                    &entry.derived_sha256,
                    &manifest.session_id,
                    Some("media-item"),
                )
                .unwrap();
        }
    }
}

impl SessionClient for Server {
    fn open_session(&self, manifest: &Manifest) -> Result<SessionPlan, Error> {
        let plan = ingest::handoff::decide(manifest, &self.ledger, &self.staging)?;
        *self.agreed.borrow_mut() = Some((manifest.clone(), plan.clone()));
        Ok(plan)
    }

    fn mark_ready(&self, _session_id: &str) -> Result<ArrivalReport, Error> {
        let agreed = self.agreed.borrow();
        let (manifest, plan) = agreed.as_ref().expect("ready before sessions");
        Ok(ingest::handoff::verify_arrivals(
            manifest,
            plan,
            &self.staging,
            &InMemoryProgress::new(),
        ))
    }
}

/// How many bytes are sitting in a directory.
fn bytes_in(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter_map(|e| e.metadata().ok())
                .filter(|m| m.is_file())
                .map(|m| m.len())
                .sum()
        })
        .unwrap_or(0)
}

struct Card4 {
    _f: Fixtures,
    card: PathBuf,
    derived: PathBuf,
    staging: PathBuf,
}

/// A card of four JPEG frames, and the two directories the handoff uses.
fn four_jpeg_card() -> Card4 {
    let f = Fixtures::new();
    let card = f.card_tree(&[
        ShotKind::JpegOnly,
        ShotKind::JpegOnly,
        ShotKind::JpegOnly,
        ShotKind::JpegOnly,
    ]);
    let derived = f.path().join("derived");
    let staging = f.path().join("staging");
    std::fs::create_dir_all(&derived).unwrap();
    std::fs::create_dir_all(&staging).unwrap();

    Card4 {
        _f: f,
        card,
        derived,
        staging,
    }
}

fn prepare(c: &Card4) -> Handoff {
    let scan = ingest::scan_card(&Card::at(&c.card).unwrap(), &InMemoryProgress::new()).unwrap();
    let (items, not_ready) = ingest::items_for(&scan, &c.derived);
    assert!(not_ready.is_empty(), "every shot here is a JPEG");
    Handoff::prepare("pending", &scan.card_id, &items).unwrap()
}

/// **Phase 11 acceptance.** Ingesting the same card twice transfers zero bytes
/// the second time and publishes nothing (F16).
#[test]
fn the_same_card_ingested_twice_transfers_nothing_the_second_time() {
    let c = four_jpeg_card();
    let server = Server::new(c.staging.clone());

    let handoff = prepare(&c);
    let first =
        ingest::run_handoff(&handoff, &c.staging, &server, &InMemoryProgress::new()).unwrap();

    assert!(first.complete());
    assert_eq!(first.copied, 4);
    assert!(first.bytes_transferred > 0);
    assert_eq!(bytes_in(&c.staging), first.bytes_transferred);

    server.publish_everything(handoff.manifest());

    // The card is put back in the reader. Everything is rescanned from scratch,
    // exactly as it would be on a second ingest.
    let again = prepare(&c);
    let second =
        ingest::run_handoff(&again, &c.staging, &server, &InMemoryProgress::new()).unwrap();

    assert_eq!(second.bytes_transferred, 0, "not one byte a second time");
    assert_eq!(second.copied, 0);
    assert_eq!(second.already_published, 4, "and nothing left to publish");
    assert_eq!(second.already_staged, 0);
    assert!(second.complete());
}

/// The hash that survives a rescan is the card's, and it is what makes the test
/// above work at all. Pinned separately so a change to what gets hashed fails
/// here, where the reason is written down, rather than only as a puzzling
/// duplicate-transfer somewhere else.
#[test]
fn a_rescan_of_the_same_card_produces_the_same_deduplication_keys() {
    let c = four_jpeg_card();

    let first: Vec<String> = prepare(&c)
        .manifest()
        .entries
        .iter()
        .map(|e| e.source_sha256.clone())
        .collect();
    let second: Vec<String> = prepare(&c)
        .manifest()
        .entries
        .iter()
        .map(|e| e.source_sha256.clone())
        .collect();

    assert_eq!(first, second);
    assert_eq!(first.len(), 4);
}

/// A card whose derivatives reached the NAS but which was never published must
/// not be transferred again — and must not be treated as finished either.
#[test]
fn a_card_staged_but_not_published_is_still_waiting_to_be_published() {
    let c = four_jpeg_card();
    let server = Server::new(c.staging.clone());

    let handoff = prepare(&c);
    ingest::run_handoff(&handoff, &c.staging, &server, &InMemoryProgress::new()).unwrap();

    let second =
        ingest::run_handoff(&prepare(&c), &c.staging, &server, &InMemoryProgress::new()).unwrap();

    assert_eq!(second.bytes_transferred, 0);
    assert_eq!(second.already_staged, 4);
    assert_eq!(second.already_published, 0);
}

/// **Phase 11 acceptance.** A truncated staged file is detected by hash and
/// recopied (specification §2.3).
#[test]
fn a_truncated_staged_file_is_recopied() {
    let c = four_jpeg_card();

    /// Damages one named file the first time `ready` is called, standing in for
    /// a copy interrupted part way over SMB.
    struct Interrupting {
        inner: Server,
        damage: RefCell<Option<String>>,
    }

    impl SessionClient for Interrupting {
        fn open_session(&self, manifest: &Manifest) -> Result<SessionPlan, Error> {
            self.inner.open_session(manifest)
        }

        fn mark_ready(&self, session_id: &str) -> Result<ArrivalReport, Error> {
            if let Some(name) = self.damage.borrow_mut().take() {
                let path = self.inner.staging.join(&name);
                let whole = std::fs::read(&path).unwrap();
                std::fs::write(&path, &whole[..whole.len() / 2]).unwrap();
            }
            self.inner.mark_ready(session_id)
        }
    }

    let handoff = prepare(&c);
    let damaged = handoff.manifest().entries[1].file_name.clone();
    let expected = std::fs::read(handoff.local_path(&damaged).unwrap()).unwrap();

    let client = Interrupting {
        inner: Server::new(c.staging.clone()),
        damage: RefCell::new(Some(damaged.clone())),
    };

    let outcome =
        ingest::run_handoff(&handoff, &c.staging, &client, &InMemoryProgress::new()).unwrap();

    assert!(outcome.complete(), "unresolved: {:?}", outcome.unresolved);
    assert_eq!(outcome.recopied, 1);
    assert_eq!(outcome.rounds, 2);
    assert_eq!(
        std::fs::read(c.staging.join(&damaged)).unwrap(),
        expected,
        "the recopied file must be the whole photograph"
    );
}

/// A RAW-only shot with no derivative is named, not dropped and not sent as a
/// RAW. Google Photos cannot show a `.CR2`, and losing the frame silently is
/// worse than saying so.
#[test]
fn a_raw_only_card_with_nothing_derived_hands_over_nothing_and_says_why() {
    let f = Fixtures::new();
    let card = f.card_tree(&[ShotKind::RawOnly, ShotKind::RawOnly]);
    let derived = f.path().join("derived");

    let scan = ingest::scan_card(&Card::at(&card).unwrap(), &InMemoryProgress::new()).unwrap();
    let (items, not_ready) = ingest::items_for(&scan, &derived);

    assert!(items.is_empty());
    assert_eq!(not_ready.len(), 2);
    assert!(not_ready[0].reason.contains("nothing was derived"));
}

/// A RAW+JPEG pair hands over the JPEG, which is what F11 already chose as the
/// candidate. No derivation needed, no RAW on the wire.
#[test]
fn a_raw_plus_jpeg_card_hands_over_the_jpegs() {
    let f = Fixtures::new();
    let card = f.card_tree(&[ShotKind::RawPlusJpeg, ShotKind::RawPlusJpeg]);
    let derived = f.path().join("derived");
    let staging = f.path().join("staging");

    let scan = ingest::scan_card(&Card::at(&card).unwrap(), &InMemoryProgress::new()).unwrap();
    let (items, not_ready) = ingest::items_for(&scan, &derived);
    assert!(not_ready.is_empty());
    assert_eq!(items.len(), 2);

    let handoff = Handoff::prepare("pending", &scan.card_id, &items).unwrap();
    let server = Server::new(staging.clone());
    let outcome =
        ingest::run_handoff(&handoff, &staging, &server, &InMemoryProgress::new()).unwrap();

    assert!(outcome.complete());
    assert_eq!(outcome.copied, 2);

    // Nothing in staging is a RAW file.
    for entry in std::fs::read_dir(&staging).unwrap() {
        let name = entry.unwrap().file_name().to_string_lossy().to_string();
        assert!(name.ends_with(".jpg"), "{name} should not be staged");
    }
}

/// The manifest describes the file that is transferred, not the one on the
/// card. A resized derivative that reported the card's 4000×3000 would make the
/// server's own record wrong from the moment it was written.
#[test]
fn the_manifest_describes_the_derivative_not_the_original() {
    let f = Fixtures::new();
    let card = f.card_tree(&[ShotKind::JpegOnly]);
    let derived = f.path().join("derived");
    std::fs::create_dir_all(&derived).unwrap();

    // Stand in for F13's resize: same stem, different dimensions.
    let resized = f.jpeg_with_exif(
        "resized.jpg",
        800,
        600,
        "2024:05:01 08:00:00",
        "CANON EOS R6",
    );
    std::fs::rename(&resized, derived.join("IMG_0000.jpg")).unwrap();

    let scan = ingest::scan_card(&Card::at(&card).unwrap(), &InMemoryProgress::new()).unwrap();
    let (items, _) = ingest::items_for(&scan, &derived);
    let handoff = Handoff::prepare("pending", &scan.card_id, &items).unwrap();

    let entry = &handoff.manifest().entries[0];
    assert_eq!((entry.width, entry.height), (800, 600));
    // The deduplication key still describes the card's file (§7).
    assert_eq!(entry.source_sha256, scan.shots[0].candidate().sha256);
}

/// The ledger survives a restart, because it is a file on disk and F16's whole
/// promise depends on it outliving the process that wrote it.
#[test]
fn the_published_ledger_survives_reopening() {
    let f = Fixtures::new();
    let path = f.path().join("ledger.sqlite3");

    {
        let ledger = Ledger::open(&path).unwrap();
        ledger
            .record_published("source-hash", "IMG_0001", "derived-hash", "s1", Some("m1"))
            .unwrap();
    }

    let reopened = Ledger::open(&path).unwrap();
    assert!(reopened.is_published("source-hash").unwrap());
    assert!(!reopened.is_published("some-other-hash").unwrap());
}
