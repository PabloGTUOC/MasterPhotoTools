//! Shot pairing and candidate selection (F11).
//!
//! F11: "Group files into shots by filename stem. A camera shooting RAW+JPEG
//! writes `IMG_1234.JPG` and `IMG_1234.CR2` for one photograph; these are one
//! shot with two assets."
//!
//! Per shot, one asset is the **candidate** — the one that will be published:
//! the JPEG if there is one, and otherwise a JPEG derived from the RAW by F14.

use crate::ingest::scanner::{AssetKind, ScannedAsset};
use std::collections::HashMap;
use std::path::Path;

/// One photograph, with every file the camera wrote for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shot {
    /// The filename stem the assets share, as the camera wrote it.
    pub stem: String,
    /// Assets in a stable order: the candidate first, then the rest by path.
    pub assets: Vec<ScannedAsset>,
    /// True when the candidate must still be produced by F14 — a RAW-only shot.
    pub needs_derivation: bool,
}

impl Shot {
    /// The asset that will be published, or that F14 will derive from.
    pub fn candidate(&self) -> &ScannedAsset {
        // Construction guarantees a non-empty asset list with the candidate
        // first, so this cannot be reached with an empty vector.
        &self.assets[0]
    }

    pub fn capture(&self) -> Option<chrono::NaiveDateTime> {
        self.candidate().capture
    }
}

/// Group scanned files into shots and pick each one's candidate (F11).
///
/// Files that are neither a photograph nor a video — a card's print-order files
/// and firmware — form no shot. They were recorded by the scan; they are simply
/// not photographs.
pub fn group_into_shots(assets: Vec<ScannedAsset>) -> Vec<Shot> {
    let mut groups: HashMap<String, Vec<ScannedAsset>> = HashMap::new();
    let mut display_stem: HashMap<String, String> = HashMap::new();

    for asset in assets {
        if asset.kind == AssetKind::Other {
            continue;
        }

        let stem = stem_of(&asset.path);
        // FAT volumes are case-insensitive, so `IMG_0001.JPG` and
        // `img_0001.cr2` are one shot. The key folds case; the reported stem is
        // whichever spelling sorts first, so it stays stable.
        let key = stem.to_ascii_uppercase();
        display_stem
            .entry(key.clone())
            .and_modify(|existing| {
                if stem < *existing {
                    *existing = stem.clone();
                }
            })
            .or_insert_with(|| stem.clone());

        groups.entry(key).or_default().push(asset);
    }

    let mut shots: Vec<Shot> = groups
        .into_iter()
        .map(|(key, mut assets)| {
            // Order within a shot: the candidate first, then by path so the
            // result does not depend on filesystem iteration order.
            assets.sort_by(|a, b| {
                candidate_rank(a)
                    .cmp(&candidate_rank(b))
                    .then_with(|| b.bytes.cmp(&a.bytes))
                    .then_with(|| a.rel_path.cmp(&b.rel_path))
            });

            let needs_derivation = assets[0].kind == AssetKind::Raw;

            Shot {
                stem: display_stem.remove(&key).unwrap_or(key),
                assets,
                needs_derivation,
            }
        })
        .collect();

    shots.sort_by(|a, b| a.stem.cmp(&b.stem));
    shots
}

/// Lower ranks win the candidate slot.
///
/// F11: a JPEG present means the JPEG is the candidate and the RAW is recorded
/// but not published; RAW only means F14 produces the candidate. A video is its
/// own candidate — nothing derives from it.
fn candidate_rank(asset: &ScannedAsset) -> u8 {
    match asset.kind {
        AssetKind::Jpeg => 0,
        AssetKind::Raw => 1,
        AssetKind::Video => 2,
        AssetKind::Other => 3,
    }
}

/// The filename stem, with a trailing sidecar extension removed.
///
/// `IMG_0001.JPG` and `IMG_0001.CR2` both give `IMG_0001`, which is what makes
/// them one shot.
fn stem_of(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn asset(name: &str, kind: AssetKind, bytes: u64) -> ScannedAsset {
        ScannedAsset {
            path: PathBuf::from(name),
            rel_path: name.to_string(),
            kind,
            bytes,
            sha256: format!("{name:0>64}"),
            width: 6000,
            height: 4000,
            capture: None,
            camera: Some("CANON EOS R6".into()),
        }
    }

    #[test]
    fn a_raw_and_a_jpeg_sharing_a_stem_are_one_shot() {
        let shots = group_into_shots(vec![
            asset("IMG_0001.CR2", AssetKind::Raw, 30_000_000),
            asset("IMG_0001.JPG", AssetKind::Jpeg, 6_000_000),
        ]);

        assert_eq!(shots.len(), 1);
        assert_eq!(shots[0].stem, "IMG_0001");
        assert_eq!(shots[0].assets.len(), 2);
    }

    #[test]
    fn the_jpeg_is_the_candidate_when_there_is_one() {
        // F11: "JPEG present → the JPEG is the candidate. The RAW is recorded
        // but not published."
        let shots = group_into_shots(vec![
            asset("IMG_0001.CR2", AssetKind::Raw, 30_000_000),
            asset("IMG_0001.JPG", AssetKind::Jpeg, 6_000_000),
        ]);

        assert_eq!(shots[0].candidate().kind, AssetKind::Jpeg);
        assert!(!shots[0].needs_derivation);
    }

    #[test]
    fn a_raw_only_shot_is_marked_for_derivation() {
        // F11: "RAW only → the candidate is produced by F14."
        let shots = group_into_shots(vec![asset("IMG_0002.CR2", AssetKind::Raw, 30_000_000)]);

        assert_eq!(shots[0].candidate().kind, AssetKind::Raw);
        assert!(shots[0].needs_derivation);
    }

    #[test]
    fn a_jpeg_only_shot_needs_nothing_derived() {
        let shots = group_into_shots(vec![asset("IMG_0003.JPG", AssetKind::Jpeg, 6_000_000)]);

        assert_eq!(shots[0].candidate().kind, AssetKind::Jpeg);
        assert!(!shots[0].needs_derivation);
    }

    #[test]
    fn case_differences_do_not_split_a_shot() {
        // FAT volumes are case-insensitive; a card written by two bodies can
        // carry both spellings.
        let shots = group_into_shots(vec![
            asset("IMG_0001.JPG", AssetKind::Jpeg, 6_000_000),
            asset("img_0001.cr2", AssetKind::Raw, 30_000_000),
        ]);

        assert_eq!(shots.len(), 1, "one photograph, not two");
        assert_eq!(shots[0].assets.len(), 2);
    }

    #[test]
    fn print_order_files_form_no_shot() {
        let shots = group_into_shots(vec![
            asset("IMG_0001.JPG", AssetKind::Jpeg, 6_000_000),
            asset("AUTPRINT.MRK", AssetKind::Other, 100),
        ]);

        assert_eq!(shots.len(), 1);
        assert_eq!(shots[0].stem, "IMG_0001");
    }

    #[test]
    fn a_video_is_its_own_candidate() {
        let shots = group_into_shots(vec![asset("MVI_0004.MOV", AssetKind::Video, 900_000_000)]);

        assert_eq!(shots[0].candidate().kind, AssetKind::Video);
        assert!(
            !shots[0].needs_derivation,
            "nothing is derived from a video"
        );
    }

    #[test]
    fn the_larger_jpeg_wins_when_a_stem_somehow_has_two() {
        // Not something a camera does, but the choice must be deterministic
        // rather than dependent on directory order.
        let shots = group_into_shots(vec![
            asset("IMG_0001.jpeg", AssetKind::Jpeg, 1_000),
            asset("IMG_0001.JPG", AssetKind::Jpeg, 9_000),
        ]);

        assert_eq!(shots.len(), 1);
        assert_eq!(shots[0].candidate().bytes, 9_000);
    }

    #[test]
    fn shots_come_back_in_a_stable_order() {
        let shots = group_into_shots(vec![
            asset("IMG_0003.JPG", AssetKind::Jpeg, 1),
            asset("IMG_0001.JPG", AssetKind::Jpeg, 1),
            asset("IMG_0002.JPG", AssetKind::Jpeg, 1),
        ]);

        let stems: Vec<&str> = shots.iter().map(|s| s.stem.as_str()).collect();
        assert_eq!(stems, vec!["IMG_0001", "IMG_0002", "IMG_0003"]);
    }

    #[test]
    fn nothing_at_all_groups_into_nothing() {
        assert!(group_into_shots(vec![]).is_empty());
    }
}
