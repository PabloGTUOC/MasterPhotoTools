//! What the ingest pipeline treats as a card (F10, build plan §6.3).
//!
//! **Simulated card mode is the only mode.** Build plan §6.3 requires that any
//! directory can be treated as a card, and the way to guarantee that is to give
//! the pipeline no other way of being told about one: detection produces a path,
//! [`Card::at`] accepts a path, and nothing below this point can tell whether a
//! real card was mounted. That keeps card *detection* — a thin, macOS-only
//! platform concern — separate from card *processing*, which is all of the work
//! and all of the tests.

use crate::error::Error;
use std::path::{Component, Path, PathBuf};

/// Where macOS mounts removable volumes. A path under here has its volume label
/// as its first component, which is what F10 reports to the user.
const MOUNT_ROOT: &str = "/Volumes";

/// The directory a camera writes into. Its presence is what makes a mounted
/// volume a *card* rather than someone's backup drive (F10).
const DCIM: &str = "DCIM";

/// A card, or a directory standing in for one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    root: PathBuf,
    label: Option<String>,
    origin: Origin,
}

/// How this card came to the pipeline's attention.
///
/// Recorded for reporting only. **No behaviour branches on it** — that is the
/// point of §6.3, and a branch here would be the coupling it forbids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// A volume mounted under `/Volumes`.
    Mounted,
    /// Any other directory, pointed at deliberately.
    Simulated,
}

impl Card {
    /// Treat `root` as a card.
    ///
    /// The directory must exist and be a directory; nothing else is required. A
    /// card with no `DCIM` is still a card here, because §6.3's second reason is
    /// re-running ingest over a folder of already-copied files, which has no
    /// `DCIM` tree. [`looks_like_a_card`](Self::looks_like_a_card) is the
    /// stricter test, and it belongs to detection.
    pub fn at(root: impl Into<PathBuf>) -> Result<Self, Error> {
        let root = root.into();

        let meta = std::fs::metadata(&root)
            .map_err(|e| Error::Config(format!("cannot read {}: {e}", root.display())))?;
        if !meta.is_dir() {
            return Err(Error::Config(format!(
                "{} is not a directory, so it cannot be treated as a card",
                root.display()
            )));
        }

        let origin = if is_mount_point(&root) {
            Origin::Mounted
        } else {
            Origin::Simulated
        };

        Ok(Self {
            label: volume_label(&root),
            root,
            origin,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn origin(&self) -> Origin {
        self.origin
    }

    /// The volume label, or the directory name for a simulated card.
    ///
    /// Part of the card's identity per F10 — the fingerprint alone would treat
    /// two identically-loaded cards as one.
    pub fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }

    /// True if this looks like a camera card rather than an arbitrary volume.
    ///
    /// This is F10's test, applied by detection *before* it notifies anyone. It
    /// is deliberately not applied by [`at`](Self::at): once a human has pointed
    /// at a directory, second-guessing them is not this type's job.
    pub fn looks_like_a_card(&self) -> bool {
        self.dcim().is_some()
    }

    /// The `DCIM` directory, whatever its case. FAT volumes are case-insensitive
    /// and cameras are not consistent about it.
    fn dcim(&self) -> Option<PathBuf> {
        let entries = std::fs::read_dir(&self.root).ok()?;
        for entry in entries.flatten() {
            if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            if entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case(DCIM)
            {
                return Some(entry.path());
            }
        }
        None
    }

    /// The subtree to walk.
    ///
    /// `DCIM` when there is one — a card's other directories hold print order
    /// files and firmware, never photographs — and otherwise the whole root, so
    /// a folder of already-copied files scans as §6.3 intends.
    pub fn media_root(&self) -> PathBuf {
        self.dcim().unwrap_or_else(|| self.root.clone())
    }
}

/// True if `path` is a volume mounted directly under the mount root.
///
/// `/Volumes/EOS_DIGITAL` is a mount; `/Volumes/EOS_DIGITAL/DCIM` is not, and
/// neither is `/Volumes` itself.
fn is_mount_point(path: &Path) -> bool {
    path.parent() == Some(Path::new(MOUNT_ROOT))
}

/// The label a human would recognise the card by.
fn volume_label(path: &Path) -> Option<String> {
    path.components()
        .next_back()
        .and_then(|c| match c {
            Component::Normal(name) => Some(name.to_string_lossy().to_string()),
            // A root directory has no name to report.
            _ => None,
        })
        .filter(|name| !name.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn any_directory_can_be_treated_as_a_card() {
        // Build plan §6.3: this is the whole requirement.
        let temp = dir();
        let card = Card::at(temp.path()).unwrap();
        assert_eq!(card.root(), temp.path());
        assert_eq!(card.origin(), Origin::Simulated);
    }

    #[test]
    fn a_directory_with_dcim_looks_like_a_card() {
        let temp = dir();
        std::fs::create_dir_all(temp.path().join("DCIM").join("100CANON")).unwrap();

        let card = Card::at(temp.path()).unwrap();
        assert!(card.looks_like_a_card());
        assert_eq!(card.media_root(), temp.path().join("DCIM"));
    }

    #[test]
    fn dcim_is_matched_whatever_its_case() {
        // FAT volumes are case-insensitive and cameras are not consistent.
        let temp = dir();
        std::fs::create_dir_all(temp.path().join("dcim")).unwrap();

        assert!(Card::at(temp.path()).unwrap().looks_like_a_card());
    }

    #[test]
    fn a_folder_of_copied_files_scans_from_its_root() {
        // §6.3's second reason: re-running ingest over already-copied files,
        // which have no DCIM tree. It must still scan, from the root.
        let temp = dir();
        std::fs::write(temp.path().join("IMG_0001.JPG"), b"x").unwrap();

        let card = Card::at(temp.path()).unwrap();
        assert!(!card.looks_like_a_card());
        assert_eq!(card.media_root(), temp.path());
    }

    #[test]
    fn the_label_is_the_directory_name() {
        let temp = dir();
        let named = temp.path().join("EOS_DIGITAL");
        std::fs::create_dir(&named).unwrap();

        assert_eq!(Card::at(&named).unwrap().label(), Some("EOS_DIGITAL"));
    }

    #[test]
    fn a_file_is_not_a_card() {
        let temp = dir();
        let file = temp.path().join("not-a-card.txt");
        std::fs::write(&file, b"x").unwrap();

        let err = Card::at(&file).unwrap_err();
        assert!(matches!(err, Error::Config(_)), "got {err:?}");
    }

    #[test]
    fn a_missing_directory_is_reported_rather_than_scanned_as_empty() {
        let temp = dir();
        let err = Card::at(temp.path().join("nowhere")).unwrap_err();
        assert!(matches!(err, Error::Config(_)), "got {err:?}");
    }

    #[test]
    fn only_a_volume_root_counts_as_mounted() {
        assert!(is_mount_point(Path::new("/Volumes/EOS_DIGITAL")));
        assert!(!is_mount_point(Path::new("/Volumes/EOS_DIGITAL/DCIM")));
        assert!(!is_mount_point(Path::new("/Volumes")));
        assert!(!is_mount_point(Path::new("/home/user/card")));
    }
}
