//! The manifest: what the desktop is about to hand over (F16, build plan task 1).
//!
//! The manifest crosses the network before any photograph does. That is the
//! whole point of F16 — *"the desktop sends content hashes in its manifest
//! **before** copying any bytes, so known duplicates are never transferred"* —
//! and it is why the manifest carries hashes rather than files.

use crate::error::Error;
use crate::ingest::scanner::hash_file;
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// One publishable photograph, as the desktop knows it before the manifest is
/// built.
///
/// `source_sha256` is the hash of the file **on the card**; `derived` is the
/// local path of the file that will actually be transferred. The two are
/// different files whenever a frame was resized or developed from RAW, and
/// conflating them breaks one of the two jobs a hash does here — see
/// [`ManifestEntry`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoffItem {
    pub stem: String,
    pub source_sha256: String,
    pub derived: PathBuf,
    pub width: u32,
    pub height: u32,
    pub capture: Option<NaiveDateTime>,
}

/// One line of the manifest.
///
/// **Two hashes, because they answer two different questions.**
///
/// - `source_sha256` is the deduplication key. Specification §7 says
///   *"`assets.sha256` is the authoritative deduplication key"*, and `assets`
///   are the files on the card — the camera's own bytes, which are never
///   rewritten and therefore hash the same on every ingest of that card. It is
///   the only hash here that is stable enough to answer "have I published this
///   photograph before?".
/// - `derived_sha256` is the transfer-verification key. It is the hash of the
///   file that crosses the network, and the only thing that can tell a
///   truncated copy from a whole one on arrival.
///
/// Deduplicating on the derived hash instead would make F16 depend on every
/// encoder in the pipeline being byte-deterministic; verifying with the source
/// hash instead is impossible, because the server never sees the card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub stem: String,
    /// The card file's hash. The deduplication key (§7).
    pub source_sha256: String,
    /// The transferred file's hash. The verification key.
    pub derived_sha256: String,
    /// The name the file takes in the staging directory.
    pub file_name: String,
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
    pub capture: Option<NaiveDateTime>,
}

impl ManifestEntry {
    /// The staged name for a derivative: its content hash and a `.jpg`.
    ///
    /// Content-addressed for the same reason [`crate::ingest::staging`] uses
    /// the same scheme — cards reuse filenames after a format, so two cards'
    /// `IMG_0001.JPG` would otherwise land on the same staged file and one
    /// photograph would quietly overwrite the other.
    pub fn staged_name(derived_sha256: &str) -> String {
        format!("{derived_sha256}.jpg")
    }

    /// Reject a file name that could escape the staging directory.
    ///
    /// The manifest arrives over the network, and the server joins `file_name`
    /// onto its staging directory. A name of `../../etc/cron.d/x` would write
    /// outside it. This is the same concern G6 addresses for request paths, at
    /// the one place in this protocol where a client names a file.
    pub fn validate(&self) -> Result<(), Error> {
        let name = Path::new(&self.file_name);

        let plain = name.file_name().map(|n| n == self.file_name.as_str());
        if !self.file_name.is_empty()
            && plain == Some(true)
            && self.file_name != ".."
            && self.file_name != "."
        {
            return Ok(());
        }

        Err(Error::AccessDenied(format!(
            "manifest entry {} names a file that is not a plain name in the \
             staging directory: {:?}",
            self.stem, self.file_name
        )))
    }
}

/// One card's worth of handoff, as it travels.
///
/// Deliberately holds no local paths: it is written by the Mac and read by the
/// NAS, and `/Users/pablo/…` means nothing on the other side.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub session_id: String,
    pub card_id: String,
    /// Unix seconds. Recorded so a stale session can be told from a fresh one.
    pub created_at: i64,
    pub entries: Vec<ManifestEntry>,
}

impl Manifest {
    pub fn total_bytes(&self) -> u64 {
        self.entries.iter().map(|e| e.bytes).sum()
    }

    pub fn entry(&self, file_name: &str) -> Option<&ManifestEntry> {
        self.entries.iter().find(|e| e.file_name == file_name)
    }

    /// Check every entry before the server acts on any of it.
    pub fn validate(&self) -> Result<(), Error> {
        if self.session_id.trim().is_empty() {
            return Err(Error::Config("a manifest needs a session id".into()));
        }
        for entry in &self.entries {
            entry.validate()?;
        }
        Ok(())
    }
}

/// A prepared handoff: the manifest to send, plus the local files it describes.
///
/// The split matters. [`Manifest`] is the wire format and carries no local
/// paths; `local` is the desktop's private map from a staged name back to the
/// file on its own disk, which is what the copy step needs and what the server
/// must never see.
#[derive(Debug, Clone)]
pub struct Handoff {
    manifest: Manifest,
    local: HashMap<String, PathBuf>,
}

impl Handoff {
    /// Hash and measure every derivative, and build the manifest from them.
    ///
    /// The derived hash is computed here rather than carried forward from
    /// [`crate::ingest::derivation`], because a derivative can also be a plain
    /// JPEG from the card that needed no work at all. One rule for both paths
    /// costs one extra read per file and removes a class of "which hash is
    /// this?" mistakes.
    pub fn prepare(
        session_id: impl Into<String>,
        card_id: impl Into<String>,
        items: &[HandoffItem],
    ) -> Result<Self, Error> {
        let mut entries = Vec::with_capacity(items.len());
        let mut local = HashMap::with_capacity(items.len());

        for item in items {
            let derived_sha256 = hash_file(&item.derived)?;
            let bytes = std::fs::metadata(&item.derived)?.len();
            let file_name = ManifestEntry::staged_name(&derived_sha256);

            local.insert(file_name.clone(), item.derived.clone());
            entries.push(ManifestEntry {
                stem: item.stem.clone(),
                source_sha256: item.source_sha256.clone(),
                derived_sha256,
                file_name,
                width: item.width,
                height: item.height,
                bytes,
                capture: item.capture,
            });
        }

        Ok(Self {
            manifest: Manifest {
                session_id: session_id.into(),
                card_id: card_id.into(),
                created_at: chrono::Utc::now().timestamp(),
                entries,
            },
            local,
        })
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Where a staged name came from on this machine.
    pub fn local_path(&self, file_name: &str) -> Option<&Path> {
        self.local.get(file_name).map(PathBuf::as_path)
    }
}

/// A shot that has nothing to hand over yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotReady {
    pub stem: String,
    pub reason: String,
}

/// Pair every shot on a card with the file that will actually be published.
///
/// Two sources, one rule. A derivative in `derived_dir` wins whenever there is
/// one, because that is the output of F13's resize or F14's ladder and it is
/// what F12's thresholds were applied to; otherwise a shot whose candidate is
/// already a JPEG hands over the camera's own file, which is the common case
/// for a frame that needed no work at all.
///
/// A RAW-only shot with no derivative is **not** an error and **not** silently
/// dropped: it is returned as [`NotReady`], because handing over the RAW would
/// publish a file Google Photos cannot show, and skipping it quietly would lose
/// the photograph.
pub fn items_for(
    scan: &crate::ingest::CardScan,
    derived_dir: &Path,
) -> (Vec<HandoffItem>, Vec<NotReady>) {
    let mut items = Vec::new();
    let mut not_ready = Vec::new();

    for shot in &scan.shots {
        let candidate = shot.candidate();
        let derivative = derived_dir.join(format!("{}.jpg", shot.stem));

        let source = if derivative.is_file() {
            derivative
        } else if candidate.kind == crate::ingest::AssetKind::Jpeg {
            candidate.path.clone()
        } else {
            not_ready.push(NotReady {
                stem: shot.stem.clone(),
                reason: format!(
                    "no JPEG for this shot: its candidate is {} and nothing was derived for it",
                    candidate.kind.as_str()
                ),
            });
            continue;
        };

        // Dimensions describe the file being transferred, which is not the file
        // on the card once F13 has resized it. Read from the derivative rather
        // than carried across from the scan, so the manifest cannot promise
        // 24 MP and deliver 10.
        let meta = crate::media::meta::read_meta(&source).ok();

        items.push(HandoffItem {
            stem: shot.stem.clone(),
            // The card's own bytes, always — this is the deduplication key, and
            // a resized derivative would give a different answer every time the
            // threshold changed.
            source_sha256: candidate.sha256.clone(),
            width: meta.as_ref().map(|m| m.width).unwrap_or(candidate.width),
            height: meta.as_ref().map(|m| m.height).unwrap_or(candidate.height),
            capture: meta.as_ref().and_then(|m| m.capture).or(candidate.capture),
            derived: source,
        });
    }

    (items, not_ready)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item_at(dir: &Path, stem: &str, contents: &[u8]) -> HandoffItem {
        let path = dir.join(format!("{stem}.jpg"));
        std::fs::write(&path, contents).unwrap();
        HandoffItem {
            stem: stem.into(),
            source_sha256: format!("{stem}-source-hash"),
            derived: path,
            width: 3000,
            height: 2000,
            capture: None,
        }
    }

    #[test]
    fn a_manifest_carries_a_hash_and_a_size_for_every_derivative() {
        let t = tempfile::tempdir().unwrap();
        let items = vec![
            item_at(t.path(), "IMG_0001", b"one"),
            item_at(t.path(), "IMG_0002", b"two two"),
        ];

        let handoff = Handoff::prepare("s1", "card-1", &items).unwrap();
        let manifest = handoff.manifest();

        assert_eq!(manifest.entries.len(), 2);
        assert_eq!(manifest.entries[0].bytes, 3);
        assert_eq!(manifest.entries[1].bytes, 7);
        assert_eq!(manifest.total_bytes(), 10);
        assert!(manifest
            .entries
            .iter()
            .all(|e| e.derived_sha256.len() == 64));
    }

    #[test]
    fn the_manifest_carries_no_local_paths() {
        // It is written on the Mac and read on the NAS. A local path would be
        // meaningless there, and would leak the photographer's directory layout.
        let t = tempfile::tempdir().unwrap();
        let items = vec![item_at(t.path(), "IMG_0001", b"one")];

        let handoff = Handoff::prepare("s1", "card-1", &items).unwrap();
        let json = serde_json::to_string(handoff.manifest()).unwrap();

        assert!(
            !json.contains(&t.path().display().to_string()),
            "the wire format must not carry the desktop's paths: {json}"
        );
        // The desktop still knows where the file is.
        let name = &handoff.manifest().entries[0].file_name;
        assert!(handoff.local_path(name).unwrap().exists());
    }

    #[test]
    fn the_manifest_round_trips_through_json_without_loss() {
        // Build plan acceptance for this phase.
        let t = tempfile::tempdir().unwrap();
        let mut items = vec![
            item_at(t.path(), "IMG_0001", b"one"),
            item_at(t.path(), "IMG_0002", b"two"),
        ];
        items[0].capture = Some(
            chrono::NaiveDate::from_ymd_opt(2024, 5, 1)
                .unwrap()
                .and_hms_opt(8, 30, 15)
                .unwrap(),
        );

        let manifest = Handoff::prepare("s1", "card-1", &items)
            .unwrap()
            .manifest()
            .clone();

        let json = serde_json::to_string(&manifest).unwrap();
        let back: Manifest = serde_json::from_str(&json).unwrap();

        assert_eq!(manifest, back);
        // Named explicitly, because a derive that silently dropped `capture`
        // would still satisfy an equality check between two empty values.
        assert_eq!(back.entries[0].capture, items[0].capture);
        assert_eq!(back.entries[1].capture, None);
    }

    #[test]
    fn identical_content_takes_the_same_staged_name() {
        let t = tempfile::tempdir().unwrap();
        let items = vec![
            item_at(t.path(), "IMG_0001", b"same"),
            item_at(t.path(), "IMG_0002", b"same"),
        ];

        let handoff = Handoff::prepare("s1", "card-1", &items).unwrap();
        let entries = &handoff.manifest().entries;

        assert_eq!(entries[0].file_name, entries[1].file_name);
    }

    #[test]
    fn different_content_takes_different_staged_names() {
        // Two cards both holding IMG_0001.JPG, different photographs.
        let t = tempfile::tempdir().unwrap();
        let items = vec![
            item_at(t.path(), "IMG_0001", b"one"),
            item_at(t.path(), "IMG_0001b", b"another"),
        ];

        let handoff = Handoff::prepare("s1", "card-1", &items).unwrap();
        let entries = &handoff.manifest().entries;

        assert_ne!(entries[0].file_name, entries[1].file_name);
    }

    #[test]
    fn a_file_name_that_would_escape_the_staging_directory_is_refused() {
        // The manifest arrives over the network; the server joins `file_name`
        // onto its staging directory.
        for hostile in ["../../etc/cron.d/x", "..", "/etc/passwd", "sub/dir.jpg", ""] {
            let entry = ManifestEntry {
                stem: "IMG_0001".into(),
                source_sha256: "a".repeat(64),
                derived_sha256: "b".repeat(64),
                file_name: hostile.into(),
                width: 1,
                height: 1,
                bytes: 1,
                capture: None,
            };
            let err = entry
                .validate()
                .expect_err("{hostile} must not be accepted as a staged name");
            assert!(matches!(err, Error::AccessDenied(_)), "for {hostile:?}");
        }
    }

    #[test]
    fn an_ordinary_staged_name_is_accepted() {
        let entry = ManifestEntry {
            stem: "IMG_0001".into(),
            source_sha256: "a".repeat(64),
            derived_sha256: "b".repeat(64),
            file_name: ManifestEntry::staged_name(&"b".repeat(64)),
            width: 1,
            height: 1,
            bytes: 1,
            capture: None,
        };
        entry.validate().unwrap();
    }

    #[test]
    fn a_manifest_validates_every_entry_not_only_the_first() {
        let t = tempfile::tempdir().unwrap();
        let items = vec![item_at(t.path(), "IMG_0001", b"one")];
        let mut manifest = Handoff::prepare("s1", "card-1", &items)
            .unwrap()
            .manifest()
            .clone();

        manifest.entries.push(ManifestEntry {
            stem: "IMG_0002".into(),
            source_sha256: "a".repeat(64),
            derived_sha256: "b".repeat(64),
            file_name: "../escape.jpg".into(),
            width: 1,
            height: 1,
            bytes: 1,
            capture: None,
        });

        assert!(manifest.validate().is_err());
    }
}

#[cfg(test)]
mod items_tests {
    use super::*;
    use crate::ingest::{scan_card, Card};
    use crate::jobs::InMemoryProgress;

    fn scan_of(dir: &Path) -> crate::ingest::CardScan {
        scan_card(&Card::at(dir).unwrap(), &InMemoryProgress::new()).unwrap()
    }

    #[test]
    fn a_jpeg_shot_with_no_derivative_hands_over_the_cards_own_file() {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join("IMG_0001.JPG"), b"a photograph").unwrap();
        let derived = t.path().join("derived");

        let (items, not_ready) = items_for(&scan_of(t.path()), &derived);

        assert_eq!(items.len(), 1);
        assert!(not_ready.is_empty());
        assert_eq!(items[0].derived, t.path().join("IMG_0001.JPG"));
    }

    #[test]
    fn a_derivative_wins_over_the_cards_own_file() {
        // F13 resized it, so the card's 24 MP original is not what gets
        // published — and the manifest must describe what is published.
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join("IMG_0001.JPG"), b"the original").unwrap();
        let derived = t.path().join("derived");
        std::fs::create_dir_all(&derived).unwrap();
        std::fs::write(derived.join("IMG_0001.jpg"), b"the resized one").unwrap();

        let (items, _) = items_for(&scan_of(t.path()), &derived);

        assert_eq!(items[0].derived, derived.join("IMG_0001.jpg"));
    }

    #[test]
    fn the_deduplication_key_stays_the_cards_hash_even_when_resized() {
        // If the derivative's hash were the key, changing MAX_MEGAPIXELS would
        // make every already-published photograph look new.
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join("IMG_0001.JPG"), b"the original").unwrap();
        let derived = t.path().join("derived");
        std::fs::create_dir_all(&derived).unwrap();
        std::fs::write(derived.join("IMG_0001.jpg"), b"the resized one").unwrap();

        let scan = scan_of(t.path());
        let (items, _) = items_for(&scan, &derived);

        assert_eq!(items[0].source_sha256, scan.shots[0].candidate().sha256);
    }

    #[test]
    fn a_raw_only_shot_with_no_derivative_is_reported_not_dropped() {
        // Handing over the RAW would publish something Google Photos cannot
        // show; skipping it silently would lose the photograph.
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join("IMG_0001.CR2"), b"raw bytes").unwrap();
        let derived = t.path().join("derived");

        let (items, not_ready) = items_for(&scan_of(t.path()), &derived);

        assert!(items.is_empty());
        assert_eq!(not_ready.len(), 1);
        assert_eq!(not_ready[0].stem, "IMG_0001");
        assert!(not_ready[0].reason.contains("nothing was derived"));
    }

    #[test]
    fn a_raw_only_shot_that_was_derived_hands_over_the_derivative() {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join("IMG_0001.CR2"), b"raw bytes").unwrap();
        let derived = t.path().join("derived");
        std::fs::create_dir_all(&derived).unwrap();
        std::fs::write(derived.join("IMG_0001.jpg"), b"developed").unwrap();

        let (items, not_ready) = items_for(&scan_of(t.path()), &derived);

        assert_eq!(items.len(), 1);
        assert!(not_ready.is_empty());
        assert_eq!(items[0].derived, derived.join("IMG_0001.jpg"));
    }
}
