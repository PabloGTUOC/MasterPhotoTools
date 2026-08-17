//! Finding the files on a card.
//!
//! Shared by the fingerprint and the scan so the two can never disagree about
//! what is on the card — a fingerprint computed over a different file set than
//! the one scanned would recognise cards incorrectly.

use crate::error::Error;
use std::path::{Path, PathBuf};
use walkdir::{DirEntry, WalkDir};

/// Directories a camera card carries that never hold photographs.
const IGNORED_DIRS: &[&str] = &[
    "System Volume Information",
    "$RECYCLE.BIN",
    ".Spotlight-V100",
    ".Trashes",
    ".fseventsd",
    "MISC",
    "PRIVATE",
    "CANONMSC",
];

/// Every real file under `root`, junk excluded, in no particular order.
///
/// Symlinks are not followed: a card is a flat tree of files, and following
/// links would let a crafted card walk out of it.
pub fn media_files(root: &Path) -> Result<Vec<PathBuf>, Error> {
    let mut out = Vec::new();

    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !is_ignored(e))
    {
        let entry =
            entry.map_err(|e| Error::Internal(format!("walking {}: {e}", root.display())))?;
        // `file_type` here is the entry's own type, not its target's, because
        // links are not followed.
        if entry.file_type().is_file() {
            out.push(entry.into_path());
        }
    }

    Ok(out)
}

/// True for entries a scan must skip.
fn is_ignored(entry: &DirEntry) -> bool {
    // The root is passed to the filter too, and excluding it would empty the
    // walk — a card directory legitimately called ".card" is still the card.
    if entry.depth() == 0 {
        return false;
    }

    let name = entry.file_name().to_string_lossy();

    // Dot files, and the AppleDouble sidecars macOS writes beside them.
    if name.starts_with('.') {
        return true;
    }

    entry.file_type().is_dir() && IGNORED_DIRS.iter().any(|d| name.eq_ignore_ascii_case(d))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn names(root: &Path) -> Vec<String> {
        let mut v: Vec<String> = media_files(root)
            .unwrap()
            .into_iter()
            .map(|p| {
                p.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        v.sort();
        v
    }

    #[test]
    fn photographs_are_found_through_nested_directories() {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir_all(t.path().join("100CANON")).unwrap();
        fs::create_dir_all(t.path().join("101CANON")).unwrap();
        fs::write(t.path().join("100CANON/IMG_0001.JPG"), b"a").unwrap();
        fs::write(t.path().join("101CANON/IMG_0002.JPG"), b"b").unwrap();

        assert_eq!(
            names(t.path()),
            vec!["100CANON/IMG_0001.JPG", "101CANON/IMG_0002.JPG"]
        );
    }

    #[test]
    fn host_and_camera_junk_is_skipped() {
        let t = tempfile::tempdir().unwrap();
        fs::write(t.path().join("IMG_0001.JPG"), b"a").unwrap();
        fs::write(t.path().join(".DS_Store"), b"junk").unwrap();
        fs::write(t.path().join("._IMG_0001.JPG"), b"junk").unwrap();
        fs::create_dir_all(t.path().join("MISC")).unwrap();
        fs::write(t.path().join("MISC/AUTPRINT.MRK"), b"junk").unwrap();
        fs::create_dir_all(t.path().join(".Trashes/501")).unwrap();
        fs::write(t.path().join(".Trashes/501/deleted.JPG"), b"junk").unwrap();

        assert_eq!(names(t.path()), vec!["IMG_0001.JPG"]);
    }

    #[test]
    fn a_symlink_out_of_the_card_is_not_followed() {
        let t = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("SECRET.JPG"), b"a").unwrap();
        fs::write(t.path().join("IMG_0001.JPG"), b"a").unwrap();

        #[cfg(unix)]
        std::os::unix::fs::symlink(outside.path(), t.path().join("escape")).unwrap();

        assert_eq!(names(t.path()), vec!["IMG_0001.JPG"]);
    }

    #[test]
    fn an_empty_card_walks_to_nothing_rather_than_failing() {
        let t = tempfile::tempdir().unwrap();
        assert!(media_files(t.path()).unwrap().is_empty());
    }
}
