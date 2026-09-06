//! The inventory: what a folder of photographs already carries.
//!
//! Read-only, and it returns its rows rather than a job id for the reason the
//! date scan does — the table *is* the answer, and handing back an id threw it
//! away. Before anything is matched or written, this is what says how much of
//! the work there is to do.

use super::exif::{self, ExifPoint};
use crate::error::Error;
use crate::media::meta::TagSource;
use crate::media::{read_meta, MediaMeta};
use crate::tools::f1_dates::is_supported_media;
use chrono::NaiveDateTime;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// What a file has, and therefore what can be done with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GeoStatus {
    /// A capture date and a position. Nothing to do.
    Ok,
    /// A date but no position: the one this tool exists for.
    NoLocation,
    /// A position but no date. Nothing to match on, and nothing to fix here.
    NoDate,
    /// Neither.
    NoDateOrLocation,
    /// Video. The position could be read; writing one is a different format
    /// and a different change.
    NotSupported,
}

impl GeoStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            GeoStatus::Ok => "ok",
            GeoStatus::NoLocation => "no location",
            GeoStatus::NoDate => "no date",
            GeoStatus::NoDateOrLocation => "no date or location",
            GeoStatus::NotSupported => "not supported",
        }
    }

    /// Whether this tool can do anything for a file in this state.
    pub fn is_actionable(&self) -> bool {
        matches!(self, GeoStatus::NoLocation)
    }
}

/// One file's row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeoScanRow {
    pub name: String,
    pub path: PathBuf,
    pub capture: Option<NaiveDateTime>,
    /// Which tag supplied the capture date.
    pub tag: Option<String>,
    /// The UTC offset the camera recorded, in minutes east, where it did.
    pub utc_offset_minutes: Option<i32>,
    /// The position already in the file, rendered the same way one written by
    /// this tool would be — so a row that has a location and a row that is
    /// about to get one read alike.
    pub location: Option<ExifPoint>,
    pub status: GeoStatus,
}

/// The counts a screen puts above the table.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeoScanSummary {
    pub total: usize,
    pub complete: usize,
    pub missing_location: usize,
    pub missing_date: usize,
    pub unsupported: usize,
}

pub fn summarise(rows: &[GeoScanRow]) -> GeoScanSummary {
    let mut summary = GeoScanSummary {
        total: rows.len(),
        ..Default::default()
    };
    for row in rows {
        match row.status {
            GeoStatus::Ok => summary.complete += 1,
            GeoStatus::NoLocation => summary.missing_location += 1,
            GeoStatus::NoDate | GeoStatus::NoDateOrLocation => summary.missing_date += 1,
            GeoStatus::NotSupported => summary.unsupported += 1,
        }
    }
    summary
}

/// The status a file's metadata puts it in.
///
/// Split out from the walk so every branch is reachable from a `MediaMeta`
/// alone, with no file behind it.
pub fn status_of(meta: &MediaMeta, is_video: bool) -> GeoStatus {
    if is_video {
        return GeoStatus::NotSupported;
    }
    match (meta.capture.is_some(), meta.gps.is_some()) {
        (true, true) => GeoStatus::Ok,
        (true, false) => GeoStatus::NoLocation,
        (false, true) => GeoStatus::NoDate,
        (false, false) => GeoStatus::NoDateOrLocation,
    }
}

fn collect_media(root: &Path, recursive: bool) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut dirs = vec![root.to_path_buf()];

    while let Some(dir) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if recursive {
                    dirs.push(path);
                }
            } else if is_supported_media(&path) {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

/// Walk a folder and report what every media file in it carries.
///
/// Read in parallel, like the date scan: the reads are independent and a card's
/// worth of frames is the normal case.
pub fn scan(root: &Path, recursive: bool) -> Result<Vec<GeoScanRow>, Error> {
    let files = collect_media(root, recursive);

    Ok(files.into_par_iter().map(|path| row(&path)).collect())
}

/// One file's row.
///
/// Shared with the matching tool, so the status a photograph is listed under is
/// computed by the same code that decides whether to act on it. Two answers to
/// one question is how a screen comes to promise something the tool then
/// declines.
pub fn row(path: &Path) -> GeoScanRow {
    let meta = read_meta(path).unwrap_or_else(|_| MediaMeta::empty());

    GeoScanRow {
        name: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string(),
        path: path.to_path_buf(),
        capture: meta.capture,
        tag: meta.capture_source.map(|t: TagSource| t.name().to_string()),
        utc_offset_minutes: meta.utc_offset_minutes,
        location: meta.gps.map(|fix| {
            exif::render(
                &super::TrackPoint {
                    // The file's capture time is the only instant available
                    // here, and it is not the fix's. The stamps this renders
                    // are therefore not read; the coordinate is what is wanted.
                    at: meta.capture.map(|c| c.and_utc().timestamp()).unwrap_or(0),
                    lat: fix.lat,
                    lon: fix.lon,
                    ele: fix.altitude,
                },
                true,
            )
        }),
        status: status_of(&meta, crate::media::is_video(path)),
    }
}

/// Expand a mix of files and folders into the media files among them.
pub fn collect_inputs(paths: &[PathBuf], recursive: bool) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = Vec::new();
    for path in paths {
        if path.is_dir() {
            files.extend(collect_media(path, recursive));
        } else if is_supported_media(path) {
            files.push(path.clone());
        }
    }
    files.sort();
    files.dedup();
    files
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::meta::GpsFix;

    fn meta(capture: bool, gps: bool) -> MediaMeta {
        let mut meta = MediaMeta::empty();
        if capture {
            meta.capture = Some(
                NaiveDateTime::parse_from_str("2026:09:04 15:33:37", "%Y:%m:%d %H:%M:%S").unwrap(),
            );
        }
        if gps {
            meta.gps = Some(GpsFix {
                lat: 52.531549,
                lon: 13.369192,
                altitude: Some(36.4),
            });
        }
        meta
    }

    #[test]
    fn a_photograph_with_both_needs_nothing_from_this_tool() {
        let status = status_of(&meta(true, true), false);
        assert_eq!(status, GeoStatus::Ok);
        assert!(!status.is_actionable());
    }

    #[test]
    fn a_photograph_with_a_date_and_no_position_is_the_one_this_tool_is_for() {
        let status = status_of(&meta(true, false), false);
        assert_eq!(status, GeoStatus::NoLocation);
        assert!(status.is_actionable());
    }

    #[test]
    fn a_photograph_with_no_date_cannot_be_matched_and_says_so() {
        // Nothing to look up. The row exists so it is visible, not so it can be
        // acted on here — the Dates tab is where that starts.
        let status = status_of(&meta(false, true), false);
        assert_eq!(status, GeoStatus::NoDate);
        assert!(!status.is_actionable());
    }

    #[test]
    fn a_photograph_with_neither_is_reported_as_having_neither() {
        assert_eq!(
            status_of(&meta(false, false), false),
            GeoStatus::NoDateOrLocation
        );
    }

    #[test]
    fn a_video_is_declined_whatever_it_carries() {
        // Declined, not omitted: a file that quietly vanishes from a listing
        // reads as a folder with fewer files in it than it has.
        assert_eq!(status_of(&meta(true, false), true), GeoStatus::NotSupported);
        assert_eq!(status_of(&meta(true, true), true), GeoStatus::NotSupported);
        assert!(!status_of(&meta(true, false), true).is_actionable());
    }

    #[test]
    fn the_summary_counts_every_row_exactly_once() {
        let rows: Vec<GeoScanRow> = [
            GeoStatus::Ok,
            GeoStatus::NoLocation,
            GeoStatus::NoLocation,
            GeoStatus::NoDate,
            GeoStatus::NoDateOrLocation,
            GeoStatus::NotSupported,
        ]
        .iter()
        .map(|status| GeoScanRow {
            name: "x.jpg".into(),
            path: PathBuf::from("x.jpg"),
            capture: None,
            tag: None,
            utc_offset_minutes: None,
            location: None,
            status: *status,
        })
        .collect();

        let summary = summarise(&rows);
        assert_eq!(summary.total, 6);
        assert_eq!(summary.complete, 1);
        assert_eq!(summary.missing_location, 2);
        assert_eq!(summary.missing_date, 2);
        assert_eq!(summary.unsupported, 1);
        assert_eq!(
            summary.complete
                + summary.missing_location
                + summary.missing_date
                + summary.unsupported,
            summary.total
        );
    }
}
