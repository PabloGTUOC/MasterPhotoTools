//! F2 — Google Takeout sidecar dates.
//!
//! Takeout writes a `.json` sidecar per media file carrying
//! `photoTakenTime.timestamp` as Unix seconds. Matching the sidecar to its media
//! file is the whole difficulty: Takeout truncates long names, and appends
//! `(1)`-style duplicate suffixes to either the media file or the sidecar.

use crate::error::Error;
use chrono::{DateTime, NaiveDateTime};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
struct TakeoutSidecar {
    #[serde(rename = "photoTakenTime")]
    photo_taken_time: Option<Timestamp>,
    #[serde(rename = "creationTime")]
    creation_time: Option<Timestamp>,
}

#[derive(Deserialize)]
struct Timestamp {
    timestamp: String,
}

impl Timestamp {
    fn to_datetime(&self) -> Option<NaiveDateTime> {
        let secs: i64 = self.timestamp.trim().parse().ok()?;
        DateTime::from_timestamp(secs, 0).map(|d| d.naive_utc())
    }
}

/// Takeout's sidecar name cap. Longer media names are truncated to this before
/// `.json` is appended.
const TRUNCATION_LIMIT: usize = 46;

/// Locate the Takeout sidecar for a media file.
///
/// Tries, in order: the exact name, a duplicate suffix moved onto the sidecar,
/// a duplicate suffix dropped entirely, and the truncated form of each. The
/// first that exists wins.
pub fn find_sidecar(media: &Path) -> Option<PathBuf> {
    let dir = media.parent()?;
    let name = media.file_name()?.to_str()?;

    for candidate in sidecar_candidates(name) {
        let path = dir.join(&candidate);
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

/// Every sidecar name Takeout might have used for `name`, most specific first.
fn sidecar_candidates(name: &str) -> Vec<String> {
    fn push(out: &mut Vec<String>, candidate: String) {
        if !out.contains(&candidate) {
            out.push(candidate);
        }
    }

    let mut out = Vec::new();

    // photo.jpg -> photo.jpg.json
    push(&mut out, format!("{name}.json"));

    // A duplicate suffix on the media file can appear on the sidecar in a
    // different place, or not at all.
    if let Some((base, suffix)) = split_duplicate_suffix(name) {
        // photo(1).jpg -> photo.jpg(1).json
        push(&mut out, format!("{base}({suffix}).json"));
        // photo(1).jpg -> photo.jpg.json
        push(&mut out, format!("{base}.json"));
    }

    // Takeout truncates long names before appending .json.
    for candidate in out.clone() {
        if let Some(stripped) = candidate.strip_suffix(".json") {
            if stripped.len() > TRUNCATION_LIMIT {
                let mut t = stripped.to_string();
                t.truncate(TRUNCATION_LIMIT);
                push(&mut out, format!("{t}.json"));
            }
        }
    }
    if name.len() > TRUNCATION_LIMIT {
        let mut t = name.to_string();
        t.truncate(TRUNCATION_LIMIT);
        push(&mut out, format!("{t}.json"));
    }

    out
}

/// Split `photo(1).jpg` into `("photo.jpg", "1")`.
fn split_duplicate_suffix(name: &str) -> Option<(String, String)> {
    let open = name.rfind('(')?;
    let close = name[open..].find(')')? + open;
    let inner = &name[open + 1..close];
    if inner.is_empty() || !inner.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let base = format!("{}{}", &name[..open], &name[close + 1..]);
    Some((base, inner.to_string()))
}

/// The capture date from a media file's Takeout sidecar, if it has one.
pub fn sidecar_date(media: &Path) -> Option<NaiveDateTime> {
    let path = find_sidecar(media)?;
    read_sidecar_date(&path)
}

/// Read a date out of a sidecar file directly.
pub fn read_sidecar_date(sidecar: &Path) -> Option<NaiveDateTime> {
    let text = fs::read_to_string(sidecar).ok()?;
    let parsed: TakeoutSidecar = serde_json::from_str(&text).ok()?;
    parsed
        .photo_taken_time
        .as_ref()
        .and_then(Timestamp::to_datetime)
        .or_else(|| {
            parsed
                .creation_time
                .as_ref()
                .and_then(Timestamp::to_datetime)
        })
}

/// One media file's sidecar status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidecarMatch {
    pub media: PathBuf,
    pub sidecar: Option<PathBuf>,
    pub date: Option<NaiveDateTime>,
}

impl SidecarMatch {
    pub fn is_resolved(&self) -> bool {
        self.date.is_some()
    }
}

/// Match sidecars across a folder, optionally recursively.
///
/// A media file with no sidecar is **reported, not fatal** — a Takeout export
/// routinely contains both.
pub fn scan_sidecars(root: &Path, recursive: bool) -> Result<Vec<SidecarMatch>, Error> {
    let mut matches = Vec::new();
    let mut dirs = vec![root.to_path_buf()];

    while let Some(dir) = dirs.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if recursive {
                    dirs.push(path);
                }
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                continue;
            }
            if !crate::tools::f1_dates::is_supported_media(&path) {
                continue;
            }

            let sidecar = find_sidecar(&path);
            let date = sidecar.as_deref().and_then(read_sidecar_date);
            matches.push(SidecarMatch {
                media: path,
                sidecar,
                date,
            });
        }
    }

    matches.sort_by(|a, b| a.media.cmp(&b.media));
    Ok(matches)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_duplicate_suffix_splits_off_the_base_name() {
        assert_eq!(
            split_duplicate_suffix("photo(1).jpg"),
            Some(("photo.jpg".to_string(), "1".to_string()))
        );
        assert_eq!(
            split_duplicate_suffix("holiday(12).jpeg"),
            Some(("holiday.jpeg".to_string(), "12".to_string()))
        );
        assert_eq!(split_duplicate_suffix("photo.jpg"), None);
        // Parentheses that are not a duplicate marker are left alone.
        assert_eq!(split_duplicate_suffix("photo(final).jpg"), None);
        assert_eq!(split_duplicate_suffix("photo().jpg"), None);
    }

    #[test]
    fn the_exact_name_is_tried_first() {
        let candidates = sidecar_candidates("photo.jpg");
        assert_eq!(candidates[0], "photo.jpg.json");
    }

    #[test]
    fn both_duplicate_suffix_placements_are_offered() {
        let candidates = sidecar_candidates("photo(1).jpg");
        assert!(candidates.contains(&"photo(1).jpg.json".to_string()));
        assert!(candidates.contains(&"photo.jpg(1).json".to_string()));
        assert!(candidates.contains(&"photo.jpg.json".to_string()));
    }

    #[test]
    fn a_long_name_offers_its_truncated_form() {
        let long = format!("{}.jpg", "a".repeat(60));
        let candidates = sidecar_candidates(&long);
        assert!(
            candidates.iter().any(|c| c.len() == TRUNCATION_LIMIT + 5),
            "expected a truncated candidate, got {candidates:?}"
        );
    }

    #[test]
    fn candidates_are_unique() {
        let candidates = sidecar_candidates("photo(1).jpg");
        let mut sorted = candidates.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), candidates.len());
    }
}
