//! Copying the frames that passed to wherever the work will happen.
//!
//! Where [`staging`](super::staging) copies to a scratch directory under
//! content-hash names, this copies to **a folder somebody chose**, under the
//! names the camera gave. That difference is the whole module: a working folder
//! is a place a person opens, and `a3f91c….jpg` is not a photograph anybody can
//! find.
//!
//! Keeping the camera's names brings back the collision staging avoids — cards
//! reuse `IMG_0001.JPG` after every format — so the rule here is **never
//! overwrite**: identical content already present is a file already delivered,
//! and different content under the same name is reported and left alone.
//!
//! > **The card is never written to (G5).** Delivery refuses outright if the
//! > destination is inside the folder being read, before a single byte is
//! > copied.

use crate::error::Error;
use crate::ingest::scanner::{hash_file, ScannedAsset};
use crate::jobs::Progress;
use std::path::{Path, PathBuf};

/// A file that arrived intact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveredFile {
    /// Where it came from. Never opened for writing.
    pub source: PathBuf,
    pub delivered: PathBuf,
    /// The hash both copies share, verified after the write.
    pub sha256: String,
    pub bytes: u64,
}

/// A file that was already there, byte for byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliverySkip {
    pub source: PathBuf,
    pub existing: PathBuf,
    pub detail: String,
}

/// A file that did not arrive, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryFailure {
    pub source: PathBuf,
    pub detail: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeliveryResult {
    pub delivered: Vec<DeliveredFile>,
    pub skipped: Vec<DeliverySkip>,
    pub failed: Vec<DeliveryFailure>,
}

impl DeliveryResult {
    /// The closing line, including when nothing happened.
    pub fn describe(&self) -> String {
        if self.delivered.is_empty() && self.failed.is_empty() {
            return match self.skipped.len() {
                0 => "Nothing to copy: no frame passed the checks".to_string(),
                n => format!("Nothing to copy: all {n} were already there"),
            };
        }

        let mut line = format!("{} copied and verified", self.delivered.len());
        if !self.skipped.is_empty() {
            line.push_str(&format!(", {} already there", self.skipped.len()));
        }
        if !self.failed.is_empty() {
            line.push_str(&format!(", {} failed", self.failed.len()));
        }
        line
    }

    pub fn all_verified(&self) -> bool {
        self.failed.is_empty()
    }
}

/// Refuse a destination that would write into the card.
///
/// **G5 in the one place it could plausibly be broken by accident.** Everything
/// else in the ingest path reads the card and writes elsewhere by construction;
/// this is the only operation whose destination a person types, and typing the
/// card is an easy mistake — it is the folder they are looking at.
///
/// Checked once, before anything is copied, and on canonicalised paths so a
/// symlink into the card is refused too.
pub fn check_destination(source_root: &Path, destination: &Path) -> Result<PathBuf, Error> {
    let source = source_root.canonicalize().map_err(|e| {
        Error::Config(format!(
            "the folder being read cannot be resolved: {} ({e})",
            source_root.display()
        ))
    })?;

    // The destination may not exist yet, which is fine and usual: resolve the
    // nearest ancestor that does, and judge that.
    let mut existing = destination.to_path_buf();
    while !existing.exists() {
        match existing.parent() {
            Some(parent) => existing = parent.to_path_buf(),
            None => {
                return Err(Error::Config(format!(
                    "{} has no existing parent to create it under",
                    destination.display()
                )))
            }
        }
    }
    let resolved = existing
        .canonicalize()
        .map_err(|e| Error::Config(format!("{} cannot be resolved ({e})", existing.display())))?;

    if resolved == source || resolved.starts_with(&source) {
        return Err(Error::Refused(format!(
            "{} is inside the card being read. The card is never written to; choose a folder \
             somewhere else.",
            destination.display()
        )));
    }

    Ok(destination.to_path_buf())
}

/// What happened to one file.
enum Outcome {
    Delivered(DeliveredFile),
    Skipped(DeliverySkip),
}

/// Copy one asset into `destination`, keeping the camera's name, and verify it.
fn deliver_asset(asset: &ScannedAsset, destination: &Path) -> Result<Outcome, Error> {
    std::fs::create_dir_all(destination)?;

    let name = asset
        .path
        .file_name()
        .ok_or_else(|| Error::Internal(format!("{} has no file name", asset.path.display())))?;
    let target = destination.join(name);

    if target.exists() {
        // Never overwrite. Identical content is a file already delivered —
        // running the same card twice is a normal thing to do. Different
        // content under the same name is two cards that both had an
        // `IMG_0001.JPG`, and silently replacing one with the other would lose
        // a photograph without anybody being told.
        let existing = hash_file(&target)?;
        if existing == asset.sha256 {
            return Ok(Outcome::Skipped(DeliverySkip {
                source: asset.path.clone(),
                existing: target,
                detail: "already there, byte for byte".into(),
            }));
        }
        return Err(Error::Refused(format!(
            "a different file called {} is already in that folder. Nothing was overwritten.",
            name.to_string_lossy()
        )));
    }

    // Through a temporary name, so an interrupted copy never leaves a file that
    // looks complete and correctly named.
    let partial = target.with_extension("partial-copy");
    std::fs::copy(&asset.path, &partial)?;
    std::fs::rename(&partial, &target)?;

    // Verified, not assumed. A card reader on a failing port returns short
    // reads rather than errors, and a truncated copy nobody checked is a
    // photograph silently lost.
    let arrived = hash_file(&target)?;
    if arrived != asset.sha256 {
        let _ = std::fs::remove_file(&target);
        return Err(Error::Internal(format!(
            "{} arrived different: expected {}, got {arrived}. Nothing was kept.",
            asset.rel_path, asset.sha256
        )));
    }

    let bytes = std::fs::metadata(&target)?.len();
    Ok(Outcome::Delivered(DeliveredFile {
        source: asset.path.clone(),
        delivered: target,
        sha256: asset.sha256.clone(),
        bytes,
    }))
}

/// Copy the given assets into `destination`, verifying each one.
///
/// A failure on one file does not abandon the rest — the other 399 frames still
/// arrive, and the one that failed is reported so it can be tried again.
pub fn deliver_all(
    assets: &[ScannedAsset],
    source_root: &Path,
    destination: &Path,
    progress: &dyn Progress,
) -> Result<DeliveryResult, Error> {
    // Before any byte is copied.
    check_destination(source_root, destination)?;

    let total = assets.len() as u64;
    let mut result = DeliveryResult::default();

    for (i, asset) in assets.iter().enumerate() {
        if progress.cancelled() {
            break;
        }

        match deliver_asset(asset, destination) {
            Ok(Outcome::Delivered(file)) => result.delivered.push(file),
            Ok(Outcome::Skipped(skip)) => result.skipped.push(skip),
            Err(e) => result.failed.push(DeliveryFailure {
                source: asset.path.clone(),
                detail: e.to_string(),
            }),
        }

        progress.report(i as u64 + 1, total, "copying");
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest::scanner::scan_files;
    use crate::jobs::InMemoryProgress;

    /// A card holding the named files, and the scan of it.
    fn card(files: &[(&str, &[u8])]) -> (tempfile::TempDir, Vec<ScannedAsset>) {
        let dir = tempfile::tempdir().unwrap();
        for (name, contents) in files {
            std::fs::write(dir.path().join(name), contents).unwrap();
        }
        let scanned = scan_files(dir.path(), &InMemoryProgress::new()).unwrap();
        let mut assets = scanned.assets;
        assets.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
        (dir, assets)
    }

    fn deliver(assets: &[ScannedAsset], from: &Path, to: &Path) -> Result<DeliveryResult, Error> {
        deliver_all(assets, from, to, &InMemoryProgress::new())
    }

    #[test]
    fn a_frame_arrives_under_the_name_the_camera_gave_it() {
        // The whole reason this is not `staging`: a working folder is a place a
        // person opens, and a content-hash filename is not a photograph
        // anybody can find.
        let (card_dir, assets) = card(&[("DSCF3841.JPG", b"\xff\xd8\xff\xe0 one")]);
        let out = tempfile::tempdir().unwrap();

        let result = deliver(&assets, card_dir.path(), out.path()).unwrap();

        assert_eq!(result.delivered.len(), 1);
        assert!(out.path().join("DSCF3841.JPG").exists());
        assert_eq!(result.delivered[0].sha256, assets[0].sha256);
    }

    #[test]
    fn what_arrives_is_byte_identical() {
        let contents = b"\xff\xd8\xff\xe0 the actual photograph".to_vec();
        let (card_dir, assets) = card(&[("DSCF3841.JPG", &contents)]);
        let out = tempfile::tempdir().unwrap();

        deliver(&assets, card_dir.path(), out.path()).unwrap();

        assert_eq!(
            std::fs::read(out.path().join("DSCF3841.JPG")).unwrap(),
            contents
        );
    }

    #[test]
    fn the_card_is_not_touched() {
        // G5, asserted rather than assumed: hashed before and after.
        let (card_dir, assets) = card(&[
            ("DSCF3841.JPG", b"\xff\xd8\xff\xe0 one"),
            ("DSCF3842.JPG", b"\xff\xd8\xff\xe0 two"),
        ]);
        let out = tempfile::tempdir().unwrap();

        let before: Vec<String> = assets.iter().map(|a| hash_file(&a.path).unwrap()).collect();
        deliver(&assets, card_dir.path(), out.path()).unwrap();
        let after: Vec<String> = assets.iter().map(|a| hash_file(&a.path).unwrap()).collect();

        assert_eq!(before, after);
        assert_eq!(std::fs::read_dir(card_dir.path()).unwrap().count(), 2);
    }

    #[test]
    fn a_destination_inside_the_card_is_refused_before_anything_is_copied() {
        // The one operation whose destination a person types, and the card is
        // the folder they are looking at. Refusing afterwards would be too
        // late: the copy would already have happened.
        let (card_dir, assets) = card(&[("DSCF3841.JPG", b"\xff\xd8\xff\xe0 one")]);
        let inside = card_dir.path().join("copies");

        let err = deliver(&assets, card_dir.path(), &inside).unwrap_err();
        assert!(matches!(err, Error::Refused(_)), "got {err}");
        assert!(!inside.exists(), "nothing should have been created");
    }

    #[test]
    fn the_card_itself_as_a_destination_is_refused() {
        let (card_dir, assets) = card(&[("DSCF3841.JPG", b"\xff\xd8\xff\xe0 one")]);
        let err = deliver(&assets, card_dir.path(), card_dir.path()).unwrap_err();
        assert!(matches!(err, Error::Refused(_)), "got {err}");
    }

    #[test]
    fn a_symlink_into_the_card_is_refused_too() {
        // The reason both sides are canonicalised.
        let (card_dir, assets) = card(&[("DSCF3841.JPG", b"\xff\xd8\xff\xe0 one")]);
        let elsewhere = tempfile::tempdir().unwrap();
        let link = elsewhere.path().join("looks-innocent");
        std::os::unix::fs::symlink(card_dir.path(), &link).unwrap();

        let err = deliver(&assets, card_dir.path(), &link).unwrap_err();
        assert!(matches!(err, Error::Refused(_)), "got {err}");
    }

    #[test]
    fn the_same_card_delivered_twice_copies_nothing_the_second_time() {
        // Running a card again is a normal thing to do — the first run was
        // interrupted, or you are not sure it finished.
        let (card_dir, assets) = card(&[("DSCF3841.JPG", b"\xff\xd8\xff\xe0 one")]);
        let out = tempfile::tempdir().unwrap();

        deliver(&assets, card_dir.path(), out.path()).unwrap();
        let again = deliver(&assets, card_dir.path(), out.path()).unwrap();

        assert_eq!(again.delivered.len(), 0);
        assert_eq!(again.skipped.len(), 1);
        assert!(again.all_verified());
    }

    #[test]
    fn a_different_photograph_of_the_same_name_is_never_overwritten() {
        // Two cards both hold IMG_0001.JPG. Replacing one with the other loses
        // a photograph, and nobody would be told.
        let (first_card, first) = card(&[("IMG_0001.JPG", b"\xff\xd8\xff\xe0 the first roll")]);
        let (second_card, second) = card(&[("IMG_0001.JPG", b"\xff\xd8\xff\xe0 a later roll")]);
        let out = tempfile::tempdir().unwrap();

        deliver(&first, first_card.path(), out.path()).unwrap();
        let clash = deliver(&second, second_card.path(), out.path()).unwrap();

        assert_eq!(clash.delivered.len(), 0);
        assert_eq!(clash.failed.len(), 1);
        assert!(clash.failed[0].detail.contains("Nothing was overwritten"));
        assert_eq!(
            std::fs::read(out.path().join("IMG_0001.JPG")).unwrap(),
            b"\xff\xd8\xff\xe0 the first roll"
        );
    }

    #[test]
    fn one_failure_does_not_abandon_the_rest() {
        let (card_dir, assets) = card(&[
            ("IMG_0001.JPG", b"\xff\xd8\xff\xe0 one"),
            ("IMG_0002.JPG", b"\xff\xd8\xff\xe0 two"),
            ("IMG_0003.JPG", b"\xff\xd8\xff\xe0 three"),
        ]);
        let out = tempfile::tempdir().unwrap();
        // A different file already occupying the middle name.
        std::fs::write(out.path().join("IMG_0002.JPG"), b"something else").unwrap();

        let result = deliver(&assets, card_dir.path(), out.path()).unwrap();

        assert_eq!(result.delivered.len(), 2);
        assert_eq!(result.failed.len(), 1);
        assert!(out.path().join("IMG_0001.JPG").exists());
        assert!(out.path().join("IMG_0003.JPG").exists());
    }

    #[test]
    fn a_destination_that_does_not_exist_yet_is_created() {
        let (card_dir, assets) = card(&[("DSCF3841.JPG", b"\xff\xd8\xff\xe0 one")]);
        let out = tempfile::tempdir().unwrap();
        let nested = out.path().join("2026/berlin");

        let result = deliver(&assets, card_dir.path(), &nested).unwrap();

        assert_eq!(result.delivered.len(), 1);
        assert!(nested.join("DSCF3841.JPG").exists());
    }

    #[test]
    fn no_partial_file_is_left_behind() {
        let (card_dir, assets) = card(&[("DSCF3841.JPG", b"\xff\xd8\xff\xe0 one")]);
        let out = tempfile::tempdir().unwrap();

        deliver(&assets, card_dir.path(), out.path()).unwrap();

        let leftovers: Vec<_> = std::fs::read_dir(out.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains("partial"))
            .collect();
        assert!(leftovers.is_empty(), "found {leftovers:?}");
    }

    #[test]
    fn a_run_that_copied_nothing_says_why_rather_than_reporting_zero() {
        let empty = DeliveryResult::default();
        assert!(
            empty.describe().contains("no frame passed"),
            "{}",
            empty.describe()
        );

        let all_present = DeliveryResult {
            skipped: vec![DeliverySkip {
                source: PathBuf::from("a.jpg"),
                existing: PathBuf::from("out/a.jpg"),
                detail: "already there".into(),
            }],
            ..Default::default()
        };
        assert!(
            all_present.describe().contains("already there"),
            "{}",
            all_present.describe()
        );
    }
}
