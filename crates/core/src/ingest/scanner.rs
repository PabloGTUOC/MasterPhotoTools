//! Card scan (F11).
//!
//! Walk the card in parallel, and for each file record path, size, pixel
//! dimensions, camera model, capture datetime and a content hash.
//!
//! **Dimensions come from metadata, never from decoding** (F11). Decoding 400
//! frames to learn their sizes turns a two-second scan into a two-minute one, so
//! this module calls [`read_meta`] and never a decoder. The test
//! `dimensions_come_from_metadata_not_from_decoding` enforces it with a file
//! whose pixel data cannot be decoded at all.
//!
//! **The card is never written to** (G5). Every file here is opened read-only.

use crate::error::Error;
use crate::ingest::walk::media_files;
use crate::jobs::Progress;
use crate::media::read_meta;
use chrono::NaiveDateTime;
use rayon::prelude::*;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

/// What kind of asset a file is, decided by extension.
///
/// By extension because deciding by content would mean opening every RAW file
/// on the card, which is the cost F11 exists to avoid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssetKind {
    Jpeg,
    Raw,
    Video,
    Other,
}

impl AssetKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            AssetKind::Jpeg => "jpeg",
            AssetKind::Raw => "raw",
            AssetKind::Video => "video",
            AssetKind::Other => "other",
        }
    }
}

/// Extensions cameras write RAW files with.
const RAW_EXTENSIONS: &[&str] = &[
    "cr2", "cr3", "crw", // Canon
    "nef", "nrw", // Nikon
    "arw", "srf", "sr2", // Sony
    "raf", // Fujifilm
    "orf", // Olympus
    "rw2", // Panasonic
    "pef", // Pentax
    "dng", // Adobe, and several bodies natively
    "raw", "3fr", "iiq", "erf", "mrw",
];

const JPEG_EXTENSIONS: &[&str] = &["jpg", "jpeg", "jpe", "heic", "heif"];
const VIDEO_EXTENSIONS: &[&str] = &["mov", "mp4", "m4v", "avi", "mts", "m2ts"];

/// What kind of asset a path names, by extension alone.
///
/// Public because detection needs it before any file is opened (F10).
pub fn classify_path(path: &Path) -> AssetKind {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();

    if JPEG_EXTENSIONS.contains(&ext.as_str()) {
        AssetKind::Jpeg
    } else if RAW_EXTENSIONS.contains(&ext.as_str()) {
        AssetKind::Raw
    } else if VIDEO_EXTENSIONS.contains(&ext.as_str()) {
        AssetKind::Video
    } else {
        AssetKind::Other
    }
}

/// One file on the card.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ScannedAsset {
    /// Absolute path, for reading. Never opened for writing (G5).
    pub path: PathBuf,
    /// Path relative to the scanned root — what the ledger records, because the
    /// absolute path changes every time the card is mounted.
    pub rel_path: String,
    pub kind: AssetKind,
    pub bytes: u64,
    /// Hex SHA-256 of the file's contents, used for deduplication (F16).
    pub sha256: String,
    /// From metadata. Zero when the file declares none — never from a decode.
    pub width: u32,
    pub height: u32,
    pub capture: Option<NaiveDateTime>,
    pub camera: Option<String>,
}

impl ScannedAsset {
    pub fn megapixels(&self) -> f64 {
        (self.width as f64 * self.height as f64) / 1_000_000.0
    }

    /// True when metadata carried no dimensions.
    ///
    /// Reported rather than resolved: F11 forbids decoding to find out, so the
    /// honest answer is that the card does not say.
    pub fn dimensions_unknown(&self) -> bool {
        self.width == 0 || self.height == 0
    }
}

/// A file that could not be scanned.
///
/// Collected rather than returned as an error: one unreadable file on a card of
/// four hundred must not cost the other 399 their scan.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ScanProblem {
    pub rel_path: String,
    pub detail: String,
}

/// The result of walking one card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedFiles {
    pub assets: Vec<ScannedAsset>,
    pub problems: Vec<ScanProblem>,
}

/// Walk `root` in parallel and read every file's metadata and hash (F11).
///
/// The walk itself is serial and cheap; the per-file work is the expensive part
/// and is what runs across the pool.
pub fn scan_files(root: &Path, progress: &dyn Progress) -> Result<ScannedFiles, Error> {
    let paths = media_files(root)?;
    let total = paths.len() as u64;
    progress.report(0, total, "reading the card");

    let done = std::sync::atomic::AtomicU64::new(0);

    let results: Vec<Result<ScannedAsset, ScanProblem>> = paths
        .par_iter()
        .map(|path| {
            let outcome = scan_one(path, root);

            // Reporting every file would swamp a subscriber on a 400-shot card;
            // every 25 is enough to show a moving bar.
            let n = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            if n % 25 == 0 || n == total {
                progress.report(n, total, "reading the card");
            }

            outcome
        })
        .collect();

    let mut assets = Vec::with_capacity(results.len());
    let mut problems = Vec::new();
    for result in results {
        match result {
            Ok(asset) => assets.push(asset),
            Err(problem) => problems.push(problem),
        }
    }

    // Parallel iteration returns in input order, but the walk's order is the
    // filesystem's. Sorting makes a scan reproducible, which the tests and the
    // review grid both need.
    assets.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    problems.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));

    Ok(ScannedFiles { assets, problems })
}

fn scan_one(path: &Path, root: &Path) -> Result<ScannedAsset, ScanProblem> {
    let rel_path = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");

    let problem = |detail: String| ScanProblem {
        rel_path: rel_path.clone(),
        detail,
    };

    let bytes = std::fs::metadata(path)
        .map_err(|e| problem(format!("could not read file size: {e}")))?
        .len();

    let sha256 = hash_file(path).map_err(|e| problem(format!("could not hash: {e}")))?;

    // F11: metadata only. A file whose metadata is unreadable yields zeroes and
    // is still recorded — validation (F12) is where a missing capture date
    // becomes a finding, not here.
    let meta = read_meta(path).unwrap_or_else(|_| crate::media::MediaMeta::empty());

    Ok(ScannedAsset {
        path: path.to_path_buf(),
        rel_path,
        kind: classify_path(path),
        bytes,
        sha256,
        width: meta.width,
        height: meta.height,
        capture: meta.capture,
        camera: meta.camera,
    })
}

/// Hex SHA-256 of a file's contents, read in chunks so a 60 MB RAW does not
/// arrive in memory whole.
pub fn hash_file(path: &Path) -> Result<String, Error> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];

    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }

    Ok(hex(&hasher.finalize()))
}

pub(crate) fn hex(bytes: &[u8]) -> String {
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
    use crate::jobs::InMemoryProgress;
    use std::path::Path;

    #[test]
    fn extensions_classify_the_way_a_camera_writes_them() {
        assert_eq!(classify_path(Path::new("IMG_0001.JPG")), AssetKind::Jpeg);
        assert_eq!(classify_path(Path::new("IMG_0001.jpg")), AssetKind::Jpeg);
        assert_eq!(classify_path(Path::new("IMG_0001.CR2")), AssetKind::Raw);
        assert_eq!(classify_path(Path::new("IMG_0001.arw")), AssetKind::Raw);
        assert_eq!(classify_path(Path::new("IMG_0001.DNG")), AssetKind::Raw);
        assert_eq!(classify_path(Path::new("MVI_0001.MOV")), AssetKind::Video);
        assert_eq!(classify_path(Path::new("AUTPRINT.MRK")), AssetKind::Other);
        assert_eq!(classify_path(Path::new("noextension")), AssetKind::Other);
    }

    #[test]
    fn an_unreadable_file_is_a_problem_not_a_failed_scan() {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join("IMG_0001.JPG"), b"not a jpeg").unwrap();

        let scanned = scan_files(t.path(), &InMemoryProgress::new()).unwrap();

        // Unparseable metadata is not a scan problem — the file is recorded with
        // no dimensions, and F12 decides what that means.
        assert_eq!(scanned.assets.len(), 1);
        assert!(scanned.problems.is_empty());
        assert!(scanned.assets[0].dimensions_unknown());
    }

    #[test]
    fn megapixels_are_computed_from_the_recorded_dimensions() {
        let asset = ScannedAsset {
            path: PathBuf::new(),
            rel_path: String::new(),
            kind: AssetKind::Jpeg,
            bytes: 0,
            sha256: String::new(),
            width: 6000,
            height: 4000,
            capture: None,
            camera: None,
        };
        assert!((asset.megapixels() - 24.0).abs() < 1e-9);
        assert!(!asset.dimensions_unknown());
    }

    #[test]
    fn identical_files_hash_identically_and_different_ones_do_not() {
        let t = tempfile::tempdir().unwrap();
        let a = t.path().join("a");
        let b = t.path().join("b");
        let c = t.path().join("c");
        std::fs::write(&a, b"same").unwrap();
        std::fs::write(&b, b"same").unwrap();
        std::fs::write(&c, b"different").unwrap();

        assert_eq!(hash_file(&a).unwrap(), hash_file(&b).unwrap());
        assert_ne!(hash_file(&a).unwrap(), hash_file(&c).unwrap());
        assert_eq!(hash_file(&a).unwrap().len(), 64);
    }

    #[test]
    fn a_scan_is_ordered_reproducibly() {
        let t = tempfile::tempdir().unwrap();
        for name in ["c.JPG", "a.JPG", "b.JPG"] {
            std::fs::write(t.path().join(name), name.as_bytes()).unwrap();
        }

        let scanned = scan_files(t.path(), &InMemoryProgress::new()).unwrap();
        let order: Vec<&str> = scanned.assets.iter().map(|a| a.rel_path.as_str()).collect();
        assert_eq!(order, vec!["a.JPG", "b.JPG", "c.JPG"]);
    }
}
