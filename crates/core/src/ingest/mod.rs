//! Card scanning, pairing, validation, remediation (F10–F14, F16)
//!
//! The pipeline takes a path and nothing else. Detection (F10) is a platform
//! concern that lives in the desktop binary and whose entire output is a path;
//! everything below runs anywhere, against any directory, which is what build
//! plan §6.3 requires and what makes Phases 8–13 testable without hardware.

pub mod card;
pub mod derivation;
pub mod fingerprint;
pub mod grouping;
pub mod remediation;
pub mod scanner;
pub mod staging;
pub mod validation;
pub mod walk;

pub use card::{Card, Origin};
pub use fingerprint::Fingerprint;
pub use grouping::{group_into_shots, Shot};
pub use remediation::{
    actions_for, apply_bulk, default_action, plan_bulk, ActionKind, BulkRequest, PlannedAction,
    RemediationParams, RemediationSummary, RemediationTool,
};
pub use scanner::{classify_path, scan_files, AssetKind, ScanProblem, ScannedAsset, ScannedFiles};
pub use staging::{stage_all, stage_asset, StagedFile, StagingFailure, StagingResult};
pub use validation::{
    validate, CardValidation, Check, CheckStatus, ClockOffset, FailureClass, Rule, ShotValidation,
};

use crate::error::Error;
use crate::jobs::Progress;
use crate::ledger::Ledger;
use scanner::hex;
use sha2::{Digest, Sha256};

/// Everything one scan of one card found (F11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardScan {
    /// Stable across remounts, so a reinserted card is recognised (F10).
    pub card_id: String,
    pub label: Option<String>,
    pub fingerprint: Fingerprint,
    pub shots: Vec<Shot>,
    /// Files that could not be read. Never fatal.
    pub problems: Vec<ScanProblem>,
}

impl CardScan {
    pub fn shot_count(&self) -> usize {
        self.shots.len()
    }

    /// Shots whose candidate must still be produced by F14.
    pub fn awaiting_derivation(&self) -> usize {
        self.shots.iter().filter(|s| s.needs_derivation).count()
    }

    /// Every asset across every shot.
    pub fn assets(&self) -> impl Iterator<Item = &ScannedAsset> {
        self.shots.iter().flat_map(|s| s.assets.iter())
    }

    /// The assets that will be published, one per shot.
    pub fn candidates(&self) -> impl Iterator<Item = &ScannedAsset> {
        self.shots.iter().map(|s| s.candidate())
    }
}

/// What detection needs to know before it interrupts anyone (F10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardSummary {
    pub card_id: String,
    pub label: Option<String>,
    /// Shots on the card.
    pub shots: usize,
    /// Shots not already recorded against this card.
    pub new_shots: usize,
    /// True if this exact card has been scanned before.
    pub seen_before: bool,
}

impl CardSummary {
    /// Whether a card is worth interrupting someone about.
    ///
    /// A card reinserted with nothing new on it raises no notification: F10's
    /// point in recognising cards is to stop announcing the same 400 shots
    /// every time the card is mounted.
    pub fn worth_announcing(&self) -> bool {
        self.new_shots > 0
    }
}

/// Look at a card without reading any photographs (F10).
///
/// Directory entries and the ledger only — no file contents, no metadata, no
/// hashing. Detection runs this on every mount, so it has to be cheap enough
/// that nobody notices it: the expensive work is [`scan_card`], and that waits
/// until someone has said yes.
pub fn summarise_card(card: &Card, ledger: &Ledger) -> Result<CardSummary, Error> {
    let fingerprint = Fingerprint::of(card)?;
    let card_id = fingerprint.card_id();

    let stems = stems_on(card)?;

    // Keyed by the physical card, not by this observation of it: the card_id
    // changes the moment a frame is shot, so asking it "have I seen this stem"
    // would answer no for every frame on every reinsertion.
    //
    // The ledger holds stems as the camera spelled them; `stems_on` folds case
    // the same way grouping does, so both sides are folded before comparison.
    let known: std::collections::HashSet<String> = ledger
        .shot_stems(&fingerprint.shot_scope())
        .map_err(|e| Error::Internal(e.to_string()))?
        .into_iter()
        .map(|s| s.to_ascii_uppercase())
        .collect();

    let new_shots = stems.iter().filter(|s| !known.contains(*s)).count();

    Ok(CardSummary {
        card_id,
        label: card.label().map(str::to_string),
        shots: stems.len(),
        new_shots,
        seen_before: !known.is_empty(),
    })
}

/// The distinct shot stems on a card, from filenames alone.
fn stems_on(card: &Card) -> Result<std::collections::BTreeSet<String>, Error> {
    let root = card.media_root();
    let mut stems = std::collections::BTreeSet::new();

    for path in walk::media_files(&root)? {
        if scanner::classify_path(&path) == AssetKind::Other {
            continue;
        }
        if let Some(stem) = path.file_stem() {
            stems.insert(stem.to_string_lossy().to_ascii_uppercase());
        }
    }

    Ok(stems)
}

/// Scan a card: fingerprint it, walk it, and pair its files into shots (F11).
///
/// Reads only. Nothing here writes to the card (G5) or to the ledger — see
/// [`record_scan`] for persistence, kept separate so a scan can be shown to
/// someone before anything is committed.
pub fn scan_card(card: &Card, progress: &dyn Progress) -> Result<CardScan, Error> {
    let fingerprint = Fingerprint::of(card)?;
    let root = card.media_root();

    let ScannedFiles { assets, problems } = scan_files(&root, progress)?;
    let shots = group_into_shots(assets);

    Ok(CardScan {
        card_id: fingerprint.card_id(),
        label: card.label().map(str::to_string),
        fingerprint,
        shots,
        problems,
    })
}

/// Write a scan to the ledger.
///
/// Idempotent: re-scanning the same card updates rows rather than duplicating
/// them, because F10's whole point is that a card can be reinserted.
pub fn record_scan(scan: &CardScan, ledger: &Ledger) -> Result<(), Error> {
    ledger
        .upsert_card(&scan.card_id, scan.label.as_deref(), &scan.fingerprint.hash)
        .map_err(|e| Error::Internal(e.to_string()))?;

    let scope = scan.fingerprint.shot_scope();

    for shot in &scan.shots {
        // Keyed by the physical card so that re-scanning after shooting more
        // frames updates the shots already recorded instead of writing a second
        // copy of every one of them under a new card_id.
        let shot_id = shot_id(&scope, &shot.stem);
        ledger
            .upsert_shot(&shot_id, &scope, &scan.card_id, &shot.stem)
            .map_err(|e| Error::Internal(e.to_string()))?;

        for (index, asset) in shot.assets.iter().enumerate() {
            let id = asset_id(&shot_id, &asset.sha256);

            ledger
                .upsert_asset(
                    &id,
                    &shot_id,
                    &asset.rel_path,
                    asset.kind.as_str(),
                    asset.bytes,
                    &asset.sha256,
                    asset.capture.map(|c| c.and_utc().timestamp()),
                    // Zero means metadata carried no dimensions; the ledger
                    // records that as unknown rather than as a 0x0 photograph.
                    (asset.width > 0).then_some(asset.width),
                    (asset.height > 0).then_some(asset.height),
                    asset.camera.as_deref(),
                )
                .map_err(|e| Error::Internal(e.to_string()))?;

            // The candidate is the first asset by construction (F11).
            if index == 0 {
                ledger
                    .set_shot_candidate(&shot_id, &id)
                    .map_err(|e| Error::Internal(e.to_string()))?;
            }
        }
    }

    Ok(())
}

/// A shot's identity: the physical card it came from and the stem the camera
/// wrote.
///
/// Derived rather than random so that re-scanning a card produces the same ids
/// and updates its rows instead of accumulating copies. Scoped to the card's
/// label rather than to `card_id`, which changes with every frame shot.
fn shot_id(scope: &str, stem: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(scope.as_bytes());
    hasher.update([0u8]);
    hasher.update(stem.as_bytes());
    hex(&hasher.finalize())
}

/// An asset's identity: its shot and its content.
fn asset_id(shot_id: &str, sha256: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(shot_id.as_bytes());
    hasher.update([0u8]);
    hasher.update(sha256.as_bytes());
    hex(&hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::InMemoryProgress;

    fn card_at(dir: &std::path::Path) -> Card {
        Card::at(dir).unwrap()
    }

    fn write(dir: &std::path::Path, name: &str, bytes: &[u8]) {
        std::fs::write(dir.join(name), bytes).unwrap();
    }

    #[test]
    fn a_scan_pairs_files_into_shots() {
        let t = tempfile::tempdir().unwrap();
        write(t.path(), "IMG_0001.JPG", b"jpeg one");
        write(t.path(), "IMG_0001.CR2", b"raw one");
        write(t.path(), "IMG_0002.JPG", b"jpeg two");

        let scan = scan_card(&card_at(t.path()), &InMemoryProgress::new()).unwrap();

        assert_eq!(scan.shot_count(), 2);
        assert_eq!(scan.awaiting_derivation(), 0);
        assert_eq!(scan.assets().count(), 3);
        assert_eq!(scan.candidates().count(), 2);
    }

    #[test]
    fn raw_only_shots_are_counted_as_awaiting_derivation() {
        let t = tempfile::tempdir().unwrap();
        write(t.path(), "IMG_0001.CR2", b"raw one");
        write(t.path(), "IMG_0002.JPG", b"jpeg two");

        let scan = scan_card(&card_at(t.path()), &InMemoryProgress::new()).unwrap();

        assert_eq!(scan.shot_count(), 2);
        assert_eq!(scan.awaiting_derivation(), 1);
    }

    #[test]
    fn the_same_card_scanned_twice_gives_the_same_identity() {
        let t = tempfile::tempdir().unwrap();
        write(t.path(), "IMG_0001.JPG", b"jpeg one");

        let first = scan_card(&card_at(t.path()), &InMemoryProgress::new()).unwrap();
        let second = scan_card(&card_at(t.path()), &InMemoryProgress::new()).unwrap();

        assert_eq!(first.card_id, second.card_id);
    }

    #[test]
    fn a_scan_is_recorded_and_re_recording_does_not_duplicate() {
        // F10: a card can be reinserted. Scanning it again must update the
        // ledger, not accumulate a second copy of every shot.
        let t = tempfile::tempdir().unwrap();
        write(t.path(), "IMG_0001.JPG", b"jpeg one");
        write(t.path(), "IMG_0001.CR2", b"raw one");
        write(t.path(), "IMG_0002.JPG", b"jpeg two");

        let ledger = Ledger::open_in_memory().unwrap();
        let scan = scan_card(&card_at(t.path()), &InMemoryProgress::new()).unwrap();

        record_scan(&scan, &ledger).unwrap();
        record_scan(&scan, &ledger).unwrap();

        let cards: i64 = ledger.count("cards").unwrap();
        let shots: i64 = ledger.count("shots").unwrap();
        let assets: i64 = ledger.count("assets").unwrap();

        assert_eq!(cards, 1);
        assert_eq!(shots, 2);
        assert_eq!(assets, 3);
    }

    #[test]
    fn every_recorded_shot_names_its_candidate() {
        let t = tempfile::tempdir().unwrap();
        write(t.path(), "IMG_0001.JPG", b"jpeg one");
        write(t.path(), "IMG_0001.CR2", b"raw one");

        let ledger = Ledger::open_in_memory().unwrap();
        let scan = scan_card(&card_at(t.path()), &InMemoryProgress::new()).unwrap();
        record_scan(&scan, &ledger).unwrap();

        assert_eq!(ledger.shots_without_candidate().unwrap(), 0);
    }

    #[test]
    fn shot_ids_differ_between_cards_carrying_the_same_filenames() {
        // Cards reuse filenames after a format. Two IMG_0001 shots from two
        // cards must not collapse into one ledger row.
        let a = shot_id("EOS_A", "IMG_0001");
        let b = shot_id("EOS_B", "IMG_0001");
        assert_ne!(a, b);
    }
}
