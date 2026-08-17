//! Copying candidates off the card (F11's invariant, G5).
//!
//! > **Invariant: the card is never written to.** Candidates are copied to a
//! > staging directory, verified by hash, and every subsequent operation acts on
//! > the copy.
//!
//! Verification is not decoration. A card reader on a failing USB port returns
//! short reads rather than errors, and a truncated copy that nobody checked is a
//! photograph silently lost. The hash the scan already computed is compared
//! against the copy, so the check costs one read of the destination.

use crate::error::Error;
use crate::ingest::scanner::{hash_file, ScannedAsset};
use crate::jobs::Progress;
use std::path::{Path, PathBuf};

/// A candidate that reached the staging directory intact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedFile {
    /// Where it came from on the card. Never opened for writing.
    pub source: PathBuf,
    pub staged: PathBuf,
    /// The hash both copies share, verified after the write.
    pub sha256: String,
    pub bytes: u64,
}

/// A candidate that did not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagingFailure {
    pub source: PathBuf,
    pub detail: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StagingResult {
    pub staged: Vec<StagedFile>,
    pub failed: Vec<StagingFailure>,
}

impl StagingResult {
    pub fn all_verified(&self) -> bool {
        self.failed.is_empty()
    }
}

/// Copy one asset into `staging_dir` and verify it arrived intact.
///
/// The staged name is the asset's content hash plus its original extension:
/// cards reuse filenames after a format, so `IMG_0001.JPG` from two cards would
/// collide, while identical content deliberately lands on the same file.
pub fn stage_asset(asset: &ScannedAsset, staging_dir: &Path) -> Result<StagedFile, Error> {
    std::fs::create_dir_all(staging_dir)?;

    let extension = asset
        .path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_else(|| "bin".into());
    let destination = staging_dir.join(format!("{}.{extension}", asset.sha256));

    // Identical content already staged is not work to redo — but it is still
    // verified, because "it was there already" is not evidence it is intact.
    if !destination.exists() {
        // Copy through a temporary name so an interrupted copy never leaves a
        // file that looks complete and correctly named.
        let partial = destination.with_extension(format!("{extension}.partial"));
        std::fs::copy(&asset.path, &partial)?;
        std::fs::rename(&partial, &destination)?;
    }

    let staged_hash = hash_file(&destination)?;
    if staged_hash != asset.sha256 {
        // Leave nothing that a later pass could mistake for a good copy.
        let _ = std::fs::remove_file(&destination);
        return Err(Error::Internal(format!(
            "staging {} produced a different file: expected {}, got {staged_hash}",
            asset.rel_path, asset.sha256
        )));
    }

    let bytes = std::fs::metadata(&destination)?.len();

    Ok(StagedFile {
        source: asset.path.clone(),
        staged: destination,
        sha256: asset.sha256.clone(),
        bytes,
    })
}

/// Stage many candidates, reporting progress and collecting failures.
///
/// A failure on one file does not abandon the rest: the copy that failed is
/// reported so Phase 11 can recopy it, and the other 399 shots still arrive.
pub fn stage_all(
    assets: &[ScannedAsset],
    staging_dir: &Path,
    progress: &dyn Progress,
) -> StagingResult {
    let total = assets.len() as u64;
    let mut result = StagingResult::default();

    for (i, asset) in assets.iter().enumerate() {
        if progress.cancelled() {
            break;
        }

        match stage_asset(asset, staging_dir) {
            Ok(staged) => result.staged.push(staged),
            Err(e) => result.failed.push(StagingFailure {
                source: asset.path.clone(),
                detail: e.to_string(),
            }),
        }

        progress.report(i as u64 + 1, total, "copying to staging");
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest::scanner::{scan_files, AssetKind};
    use crate::jobs::InMemoryProgress;

    fn card_with(name: &str, contents: &[u8]) -> (tempfile::TempDir, ScannedAsset) {
        let card = tempfile::tempdir().unwrap();
        std::fs::write(card.path().join(name), contents).unwrap();
        let scanned = scan_files(card.path(), &InMemoryProgress::new()).unwrap();
        let asset = scanned.assets.into_iter().next().unwrap();
        (card, asset)
    }

    #[test]
    fn a_candidate_is_copied_and_verified() {
        let (_card, asset) = card_with("IMG_0001.JPG", b"photograph");
        let staging = tempfile::tempdir().unwrap();

        let staged = stage_asset(&asset, staging.path()).unwrap();

        assert!(staged.staged.exists());
        assert_eq!(staged.sha256, asset.sha256);
        assert_eq!(std::fs::read(&staged.staged).unwrap(), b"photograph");
    }

    #[test]
    fn the_card_is_not_written_to() {
        // G5, and F11's stated invariant. The card directory is made read-only,
        // so any write attempt fails rather than passing unnoticed.
        let (card, asset) = card_with("IMG_0001.JPG", b"photograph");
        let staging = tempfile::tempdir().unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(card.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
        }

        let staged = stage_asset(&asset, staging.path()).unwrap();
        assert!(staged.staged.exists());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(card.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        // The card still holds exactly what it held.
        let remaining: Vec<_> = std::fs::read_dir(card.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(remaining, vec![std::ffi::OsString::from("IMG_0001.JPG")]);
    }

    #[test]
    fn staging_the_same_content_twice_lands_on_one_file() {
        let (_card, asset) = card_with("IMG_0001.JPG", b"photograph");
        let staging = tempfile::tempdir().unwrap();

        let first = stage_asset(&asset, staging.path()).unwrap();
        let second = stage_asset(&asset, staging.path()).unwrap();

        assert_eq!(first.staged, second.staged);
        assert_eq!(std::fs::read_dir(staging.path()).unwrap().count(), 1);
    }

    #[test]
    fn a_corrupted_copy_is_detected_and_removed() {
        // The hash the scan computed no longer matches the file on the card, so
        // the copy cannot verify. This is the truncated-read case.
        let (_card, mut asset) = card_with("IMG_0001.JPG", b"photograph");
        let staging = tempfile::tempdir().unwrap();

        asset.sha256 = "0".repeat(64);
        let err = stage_asset(&asset, staging.path()).unwrap_err();

        assert!(err.to_string().contains("different file"), "got {err}");
        assert_eq!(
            std::fs::read_dir(staging.path()).unwrap().count(),
            0,
            "a file that failed verification must not be left behind"
        );
    }

    #[test]
    fn no_partial_file_survives_a_completed_stage() {
        let (_card, asset) = card_with("IMG_0001.JPG", b"photograph");
        let staging = tempfile::tempdir().unwrap();

        stage_asset(&asset, staging.path()).unwrap();

        let partials = std::fs::read_dir(staging.path())
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .contains("partial")
            })
            .count();
        assert_eq!(partials, 0);
    }

    #[test]
    fn staging_many_reports_what_arrived_and_what_did_not() {
        let card = tempfile::tempdir().unwrap();
        for i in 0..3 {
            std::fs::write(
                card.path().join(format!("IMG_000{i}.JPG")),
                format!("photograph {i}").as_bytes(),
            )
            .unwrap();
        }
        let staging = tempfile::tempdir().unwrap();
        let mut assets = scan_files(card.path(), &InMemoryProgress::new())
            .unwrap()
            .assets;

        // One asset carries a hash that cannot match, standing in for a bad read.
        assets[1].sha256 = "0".repeat(64);

        let result = stage_all(&assets, staging.path(), &InMemoryProgress::new());

        assert_eq!(result.staged.len(), 2, "the good copies still arrive");
        assert_eq!(result.failed.len(), 1);
        assert!(!result.all_verified());
    }

    #[test]
    fn the_staged_name_does_not_collide_across_cards() {
        // Two cards, both with IMG_0001.JPG, different photographs.
        let staging = tempfile::tempdir().unwrap();
        let (_a, first) = card_with("IMG_0001.JPG", b"one");
        let (_b, second) = card_with("IMG_0001.JPG", b"two");

        let one = stage_asset(&first, staging.path()).unwrap();
        let two = stage_asset(&second, staging.path()).unwrap();

        assert_ne!(one.staged, two.staged);
        assert_eq!(std::fs::read(&one.staged).unwrap(), b"one");
        assert_eq!(std::fs::read(&two.staged).unwrap(), b"two");
    }

    #[test]
    fn an_extensionless_file_still_stages() {
        let (_card, asset) = card_with("IMG_0001", b"photograph");
        let staging = tempfile::tempdir().unwrap();

        let staged = stage_asset(&asset, staging.path()).unwrap();
        assert_eq!(asset.kind, AssetKind::Other);
        assert!(staged.staged.to_string_lossy().ends_with(".bin"));
    }
}
