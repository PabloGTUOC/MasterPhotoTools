//! Recognising a card that has been seen before (F10).
//!
//! F10: "A card is identified by its volume label plus a fingerprint computed
//! over the sorted `(relative path, size, modification time)` tuples of its
//! contents, so a reinserted card is recognised as one already seen."
//!
//! Sorted, because directory iteration order is not stable across filesystems or
//! mounts. Relative, because the same card mounts at different absolute paths.
//! Size and mtime rather than content, because hashing every byte of a 64 GB
//! card to decide whether it is new would cost more than the scan it precedes.

use crate::error::Error;
use crate::ingest::card::Card;
use crate::ingest::walk::media_files;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::UNIX_EPOCH;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fingerprint {
    /// Hex SHA-256 over the sorted content tuples.
    pub hash: String,
    /// The card's label, part of its identity alongside the hash.
    pub volume_label: Option<String>,
}

impl Fingerprint {
    /// Fingerprint a card by walking its media tree.
    pub fn of(card: &Card) -> Result<Self, Error> {
        let root = card.media_root();
        let mut tuples: Vec<(String, u64, i64)> = Vec::new();

        for path in media_files(&root)? {
            let meta = std::fs::metadata(&path)?;
            tuples.push((relative(&path, &root), meta.len(), mtime_secs(&meta)));
        }

        // Directory order is not stable; the fingerprint must be.
        tuples.sort();

        let mut hasher = Sha256::new();
        for (rel, size, mtime) in &tuples {
            // Length-prefixed so that no two different tuple lists can produce
            // the same byte stream.
            hasher.update((rel.len() as u64).to_le_bytes());
            hasher.update(rel.as_bytes());
            hasher.update(size.to_le_bytes());
            hasher.update(mtime.to_le_bytes());
        }

        Ok(Self {
            hash: hex(&hasher.finalize()),
            volume_label: card.label().map(str::to_string),
        })
    }

    /// The card's identity: label and contents together, exactly as F10 states.
    ///
    /// Two cards loaded with identical files are still two cards, so the label
    /// participates. A card that is relabelled is a new card, which is the
    /// safe direction to be wrong in — it re-scans rather than silently
    /// treating a different card as already ingested.
    ///
    /// **This changes every time a frame is shot**, because the contents are
    /// part of it. That is what F10 specifies and it is right for the `cards`
    /// table, which records observed states of a card. It is the wrong key for
    /// asking "have I seen this photograph before", because the answer would be
    /// no every time anything was added — see [`shot_scope`](Self::shot_scope).
    pub fn card_id(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.volume_label.as_deref().unwrap_or("").as_bytes());
        hasher.update([0u8]);
        hasher.update(self.hash.as_bytes());
        hex(&hasher.finalize())
    }

    /// What identifies the *physical* card across shooting sessions.
    ///
    /// The label, which survives shooting more frames — unlike
    /// [`card_id`](Self::card_id), which by construction does not. Shots are
    /// keyed by this so that reinserting a card after shooting forty more frames
    /// reports forty new shots rather than all four hundred.
    ///
    /// A card with no label has nothing that survives a change of contents, so
    /// it falls back to the content hash and every new state reads as a new
    /// card. Volumes always have a name on macOS; this is the honest answer for
    /// the case where one somehow does not.
    pub fn shot_scope(&self) -> String {
        self.volume_label
            .clone()
            .unwrap_or_else(|| format!("fingerprint:{}", self.hash))
    }
}

fn relative(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Modification time in whole seconds since the epoch.
///
/// Whole seconds because sub-second precision does not survive a FAT round trip,
/// and a fingerprint that changed depending on the filesystem would defeat the
/// purpose. A time before the epoch, or one the platform cannot report, is
/// recorded as 0 rather than failing the scan.
fn mtime_secs(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    struct Tree {
        temp: tempfile::TempDir,
    }

    impl Tree {
        fn new(name: &str) -> Self {
            let temp = tempfile::tempdir().unwrap();
            fs::create_dir_all(temp.path().join(name).join("DCIM")).unwrap();
            Self { temp }
        }

        fn card(&self, name: &str) -> Card {
            Card::at(self.temp.path().join(name)).unwrap()
        }

        fn dcim(&self, name: &str) -> PathBuf {
            self.temp.path().join(name).join("DCIM")
        }
    }

    fn write(path: PathBuf, bytes: &[u8]) {
        fs::write(path, bytes).unwrap();
    }

    #[test]
    fn the_same_card_fingerprints_the_same_way_twice() {
        // F10: a reinserted card must be recognised as one already seen.
        let t = Tree::new("EOS");
        write(t.dcim("EOS").join("IMG_0001.JPG"), b"aaaa");
        write(t.dcim("EOS").join("IMG_0002.JPG"), b"bbbb");

        let first = Fingerprint::of(&t.card("EOS")).unwrap();
        let second = Fingerprint::of(&t.card("EOS")).unwrap();

        assert_eq!(first, second);
        assert_eq!(first.card_id(), second.card_id());
    }

    #[test]
    fn adding_a_file_changes_the_fingerprint() {
        let t = Tree::new("EOS");
        write(t.dcim("EOS").join("IMG_0001.JPG"), b"aaaa");
        let before = Fingerprint::of(&t.card("EOS")).unwrap();

        write(t.dcim("EOS").join("IMG_0002.JPG"), b"bbbb");
        let after = Fingerprint::of(&t.card("EOS")).unwrap();

        assert_ne!(
            before.hash, after.hash,
            "a new shot must be a new fingerprint"
        );
    }

    #[test]
    fn changing_a_files_size_changes_the_fingerprint() {
        let t = Tree::new("EOS");
        write(t.dcim("EOS").join("IMG_0001.JPG"), b"aaaa");
        let before = Fingerprint::of(&t.card("EOS")).unwrap();

        write(t.dcim("EOS").join("IMG_0001.JPG"), b"aaaaaaaa");
        let after = Fingerprint::of(&t.card("EOS")).unwrap();

        assert_ne!(before.hash, after.hash);
    }

    #[test]
    fn two_cards_holding_identical_files_are_still_two_cards() {
        // The content hash matches; the label is what separates them, which is
        // why F10 names both.
        let t = Tree::new("EOS_A");
        fs::create_dir_all(t.temp.path().join("EOS_B").join("DCIM")).unwrap();
        write(t.dcim("EOS_A").join("IMG_0001.JPG"), b"aaaa");
        write(t.dcim("EOS_B").join("IMG_0001.JPG"), b"aaaa");

        let a = Fingerprint::of(&t.card("EOS_A")).unwrap();
        let b = Fingerprint::of(&t.card("EOS_B")).unwrap();

        assert_eq!(a.hash, b.hash, "identical contents hash identically");
        assert_ne!(a.card_id(), b.card_id(), "but they are not the same card");
    }

    #[test]
    fn the_fingerprint_does_not_depend_on_where_the_card_is_mounted() {
        // The same card mounts at different absolute paths; only the label and
        // the relative layout may participate.
        let one = tempfile::tempdir().unwrap();
        let two = tempfile::tempdir().unwrap();
        for base in [one.path(), two.path()] {
            fs::create_dir_all(base.join("EOS").join("DCIM").join("100CANON")).unwrap();
            write(base.join("EOS/DCIM/100CANON/IMG_0001.JPG"), b"aaaa");
        }

        let a = Fingerprint::of(&Card::at(one.path().join("EOS")).unwrap()).unwrap();
        let b = Fingerprint::of(&Card::at(two.path().join("EOS")).unwrap()).unwrap();

        assert_eq!(a.card_id(), b.card_id());
    }

    #[test]
    fn junk_the_host_writes_does_not_change_the_card() {
        // macOS drops .DS_Store onto any volume it browses. The card is not
        // written to by us (G5), but it is written to, and a fingerprint that
        // changed because Finder looked at it would defeat recognition.
        let t = Tree::new("EOS");
        write(t.dcim("EOS").join("IMG_0001.JPG"), b"aaaa");
        let before = Fingerprint::of(&t.card("EOS")).unwrap();

        write(t.dcim("EOS").join(".DS_Store"), b"junk");
        write(t.dcim("EOS").join("._IMG_0001.JPG"), b"junk");
        let after = Fingerprint::of(&t.card("EOS")).unwrap();

        assert_eq!(before, after);
    }

    #[test]
    fn an_empty_card_still_fingerprints() {
        let t = Tree::new("EOS");
        let f = Fingerprint::of(&t.card("EOS")).unwrap();
        assert_eq!(f.hash.len(), 64);
        assert_eq!(f.volume_label.as_deref(), Some("EOS"));
    }
}
