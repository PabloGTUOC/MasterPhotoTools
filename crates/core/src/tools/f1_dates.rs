//! F1 — date scan and repair.

use crate::error::Error;
use crate::jobs::{Outcome, Progress, ToolResult};
use crate::media::meta::{read_meta, DateSet, ExifWriter, TagSource};
use crate::tools::{Plan, Skip, Tool};
use chrono::{DateTime, Days, Months, NaiveDateTime, Utc};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Extensions F1 handles, grouped as the specification lists them.
pub const IMAGE_EXTENSIONS: [&str; 8] =
    ["jpg", "jpeg", "png", "gif", "tif", "tiff", "heic", "heif"];
pub const RAW_EXTENSIONS: [&str; 5] = ["dng", "nef", "arw", "cr2", "raf"];
pub const VIDEO_EXTENSIONS: [&str; 7] = ["mov", "mp4", "m4v", "mts", "m2ts", "3gp", "avi"];

/// How far a metadata date may sit from the filesystem date before the file is
/// reported as `Mismatch`.
pub const MISMATCH_TOLERANCE_SECONDS: i64 = 60;

pub fn is_supported_media(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    let ext = ext.to_lowercase();
    IMAGE_EXTENSIONS.contains(&ext.as_str())
        || RAW_EXTENSIONS.contains(&ext.as_str())
        || VIDEO_EXTENSIONS.contains(&ext.as_str())
}

// ---------------------------------------------------------------------------
// Platform-dependent filesystem timestamps
// ---------------------------------------------------------------------------

/// Which filesystem timestamp a reading actually came from.
///
/// macOS and BSD expose a creation ("birth") time; Linux does not expose one
/// that can be set. Reporting a modification time as though it were a creation
/// time would be reporting an outcome that was not verified (§9.2 invariant 6),
/// so the source travels with the value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FsTimeSource {
    /// A real birth time.
    Created,
    /// No birth time available; this is the modification time.
    Modified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FsTime {
    pub at: NaiveDateTime,
    pub source: FsTimeSource,
}

/// True where the platform has a creation time that can be **written**.
pub const fn birth_time_is_settable() -> bool {
    cfg!(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    ))
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
))]
fn birth_time(metadata: &fs::Metadata) -> Option<SystemTime> {
    metadata.created().ok()
}

#[cfg(not(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
)))]
fn birth_time(_metadata: &fs::Metadata) -> Option<SystemTime> {
    // Linux has no birth time this code may rely on setting.
    None
}

fn to_naive(t: SystemTime) -> NaiveDateTime {
    let dt: DateTime<Utc> = t.into();
    dt.naive_utc()
}

/// The filesystem timestamp for a path, and which timestamp it is.
pub fn fs_time(path: &Path) -> Option<FsTime> {
    let metadata = fs::metadata(path).ok()?;
    if let Some(created) = birth_time(&metadata) {
        return Some(FsTime {
            at: to_naive(created),
            source: FsTimeSource::Created,
        });
    }
    Some(FsTime {
        at: to_naive(metadata.modified().ok()?),
        source: FsTimeSource::Modified,
    })
}

/// The modification time alone, used to verify what a write actually did.
pub fn modified_time(path: &Path) -> Option<NaiveDateTime> {
    fs::metadata(path).ok()?.modified().ok().map(to_naive)
}

// ---------------------------------------------------------------------------
// Scan
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DateStatus {
    Ok,
    Mismatch,
    MissingMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    pub name: String,
    pub path: PathBuf,
    pub metadata_date: Option<NaiveDateTime>,
    /// Which of F1's seven tags supplied the date.
    pub tag: Option<String>,
    pub fs_date: Option<NaiveDateTime>,
    /// Whether `fs_date` is a birth time or a modification time.
    pub fs_date_source: Option<FsTimeSource>,
    pub status: DateStatus,
}

fn collect_media(root: &Path, recursive: bool) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut dirs = vec![root.to_path_buf()];

    while let Some(dir) = dirs.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
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

/// Walk a folder and report the date state of every media file.
///
/// Metadata is read in parallel: the §9.1 target is 500 files in under five
/// seconds, and the reads are independent.
pub fn scan_dates(root: &Path, recursive: bool) -> Result<Vec<ScanResult>, Error> {
    let files = collect_media(root, recursive);

    Ok(files
        .into_par_iter()
        .map(|path| {
            let meta = read_meta(&path).unwrap_or_else(|_| crate::media::MediaMeta::empty());
            let fs = fs_time(&path);

            let status = match (meta.capture, fs.as_ref()) {
                (None, _) => DateStatus::MissingMetadata,
                (Some(m), Some(f)) => {
                    let drift = (m.and_utc().timestamp() - f.at.and_utc().timestamp()).abs();
                    if drift > MISMATCH_TOLERANCE_SECONDS {
                        DateStatus::Mismatch
                    } else {
                        DateStatus::Ok
                    }
                }
                // A date we cannot compare against is not a date we can call OK.
                (Some(_), None) => DateStatus::Mismatch,
            };

            ScanResult {
                name: path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
                metadata_date: meta.capture,
                tag: meta.capture_source.map(|t: TagSource| t.name().to_string()),
                fs_date: fs.map(|f| f.at),
                fs_date_source: fs.map(|f| f.source),
                status,
                path,
            }
        })
        .collect())
}

// ---------------------------------------------------------------------------
// Shift deltas
// ---------------------------------------------------------------------------

/// A shift delta in exiftool's `Y:M:D h:m:s` form, e.g. `+1:0:0 0:0:0`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShiftDelta {
    pub negative: bool,
    pub years: u32,
    pub months: u32,
    pub days: u64,
    pub hours: i64,
    pub minutes: i64,
    pub seconds: i64,
}

impl ShiftDelta {
    /// Parse `[+|-]Y:M:D h:m:s`. The time half may be omitted.
    pub fn parse(raw: &str) -> Result<Self, Error> {
        let raw = raw.trim();
        let (negative, body) = match raw.strip_prefix('-') {
            Some(rest) => (true, rest.trim()),
            None => (false, raw.trim_start_matches('+').trim()),
        };

        let mut halves = body.split_whitespace();
        let date = halves.next().unwrap_or("0:0:0");
        let time = halves.next().unwrap_or("0:0:0");
        if halves.next().is_some() {
            return Err(Error::Config(format!("Malformed shift delta {raw:?}")));
        }

        let parse3 = |s: &str, what: &str| -> Result<(i64, i64, i64), Error> {
            let parts: Vec<&str> = s.split(':').collect();
            if parts.len() != 3 {
                return Err(Error::Config(format!(
                    "Malformed {what} in shift delta {raw:?}: expected three colon-separated values"
                )));
            }
            let mut out = [0i64; 3];
            for (i, p) in parts.iter().enumerate() {
                out[i] = p.trim().parse().map_err(|_| {
                    Error::Config(format!("Malformed {what} in shift delta {raw:?}"))
                })?;
            }
            Ok((out[0], out[1], out[2]))
        };

        let (y, mo, d) = parse3(date, "date part")?;
        let (h, mi, s) = parse3(time, "time part")?;
        if y < 0 || mo < 0 || d < 0 || h < 0 || mi < 0 || s < 0 {
            return Err(Error::Config(format!(
                "Shift delta {raw:?} must carry its sign as a leading + or -, not per field"
            )));
        }

        Ok(Self {
            negative,
            years: y as u32,
            months: mo as u32,
            days: d as u64,
            hours: h,
            minutes: mi,
            seconds: s,
        })
    }

    /// The magnitude in exiftool's syntax, without a sign.
    pub fn magnitude(&self) -> String {
        format!(
            "{}:{}:{} {}:{}:{}",
            self.years, self.months, self.days, self.hours, self.minutes, self.seconds
        )
    }

    /// Apply the delta, so a plan can state what the result will be.
    pub fn apply(&self, at: NaiveDateTime) -> Option<NaiveDateTime> {
        let months = Months::new(self.years * 12 + self.months);
        let days = Days::new(self.days);
        let clock = chrono::Duration::hours(self.hours)
            + chrono::Duration::minutes(self.minutes)
            + chrono::Duration::seconds(self.seconds);

        if self.negative {
            at.checked_sub_months(months)?
                .checked_sub_days(days)?
                .checked_sub_signed(clock)
        } else {
            at.checked_add_months(months)?
                .checked_add_days(days)?
                .checked_add_signed(clock)
        }
    }
}

// ---------------------------------------------------------------------------
// Repair
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RepairMode {
    /// Take the best available metadata date.
    Auto,
    /// Force a supplied date.
    Manual(NaiveDateTime),
    /// Offset all dates by a delta, e.g. `+1:0:0 0:0:0`.
    Shift(String),
    /// Take the date from a Google Takeout sidecar (F2).
    Sidecar,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DateRepairParams {
    pub paths: Vec<PathBuf>,
    pub mode: RepairMode,
    /// Whether a folder among `paths` contributes its subfolders too.
    ///
    /// The scan path has always walked folders (`collect_media`); the repair
    /// path treated every input as a file, so a folder reached `read_meta`,
    /// failed, and was reported as skipped — the tool doing nothing at all on
    /// the most obvious thing to point it at.
    #[serde(default)]
    pub recursive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DateRepairAction {
    pub path: PathBuf,
    /// What the file's date will read after applying.
    pub new_date: NaiveDateTime,
    /// Set for `shift`, which is applied as a relative delta rather than an
    /// absolute stamp so exiftool handles per-tag arithmetic.
    pub shift: Option<String>,
}

/// What a single repair actually achieved.
///
/// Every field is set from a re-read after the write, never from the fact that
/// the command was issued (§9.2 invariant 6).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DateRepairOutcome {
    pub path: PathBuf,
    pub intended: NaiveDateTime,
    /// The metadata date now reads as intended.
    pub metadata_verified: bool,
    /// The filesystem modification time now reads as intended.
    pub filesystem_verified: bool,
    /// Anything the caller must know that is not a plain success.
    pub note: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DateRepairSummary {
    pub outcomes: Vec<DateRepairOutcome>,
    pub failures: Vec<(PathBuf, String)>,
}

impl DateRepairSummary {
    /// True only when every file was written *and* the write was confirmed.
    pub fn fully_verified(&self) -> bool {
        self.failures.is_empty()
            && self
                .outcomes
                .iter()
                .all(|o| o.metadata_verified && o.filesystem_verified)
    }

    pub fn verified_count(&self) -> usize {
        self.outcomes
            .iter()
            .filter(|o| o.metadata_verified && o.filesystem_verified)
            .count()
    }
}

pub struct DateRepairTool;

impl Tool for DateRepairTool {
    type Params = DateRepairParams;
    type Action = DateRepairAction;
    type Summary = DateRepairSummary;

    /// Dry run. Touches no file.
    fn plan(&self, p: &Self::Params) -> ToolResult<Plan<Self::Action>> {
        let mut actions = Vec::new();
        let mut skipped = Vec::new();

        // Validate the delta once, so a malformed one fails the whole plan
        // rather than silently skipping every file.
        let delta = match &p.mode {
            RepairMode::Shift(raw) => Some(ShiftDelta::parse(raw)?),
            _ => None,
        };

        // A folder contributes the media files inside it, exactly as it does
        // for a scan. Anything that is not media is not silently dropped: it
        // never entered the list, and a folder with nothing in it reports so.
        let mut inputs: Vec<PathBuf> = Vec::new();
        for path in &p.paths {
            if !path.exists() {
                skipped.push(Skip {
                    file: path.to_string_lossy().to_string(),
                    reason: "File not found".into(),
                });
                continue;
            }

            if path.is_dir() {
                let found = collect_media(path, p.recursive);
                if found.is_empty() {
                    skipped.push(Skip {
                        file: path.to_string_lossy().to_string(),
                        reason: "Folder holds no media this tool reads".into(),
                    });
                }
                inputs.extend(found);
                continue;
            }

            inputs.push(path.clone());
        }

        for path in &inputs {
            let resolved = match &p.mode {
                RepairMode::Manual(dt) => Some((*dt, None)),
                RepairMode::Auto => read_meta(path)
                    .ok()
                    .and_then(|m| m.capture)
                    .map(|dt| (dt, None)),
                RepairMode::Shift(_) => {
                    let delta = delta.as_ref().expect("validated above");
                    read_meta(path)
                        .ok()
                        .and_then(|m| m.capture)
                        .and_then(|dt| delta.apply(dt))
                        .map(|dt| (dt, Some(delta.clone())))
                }
                RepairMode::Sidecar => {
                    crate::tools::f2_takeout::sidecar_date(path).map(|dt| (dt, None))
                }
            };

            match resolved {
                Some((new_date, delta)) => actions.push(DateRepairAction {
                    path: path.clone(),
                    new_date,
                    shift: delta.map(|d| {
                        if d.negative {
                            format!("-{}", d.magnitude())
                        } else {
                            format!("+{}", d.magnitude())
                        }
                    }),
                }),
                None => skipped.push(Skip {
                    file: path.to_string_lossy().to_string(),
                    reason: match &p.mode {
                        RepairMode::Auto => "No metadata date to copy".into(),
                        RepairMode::Shift(_) => "No metadata date to shift".into(),
                        RepairMode::Sidecar => "No Takeout sidecar found".into(),
                        RepairMode::Manual(_) => unreachable!("manual always resolves"),
                    },
                }),
            }
        }

        Ok(Outcome {
            data: Plan { actions, skipped },
        })
    }

    fn apply(
        &self,
        plan: Plan<Self::Action>,
        progress: &dyn Progress,
    ) -> ToolResult<Self::Summary> {
        let total = plan.actions.len() as u64;
        let mut summary = DateRepairSummary::default();
        if total == 0 {
            return Ok(Outcome { data: summary });
        }

        // One writer for the whole batch (G4).
        let mut writer = ExifWriter::start()?;

        for (done, action) in plan.actions.into_iter().enumerate() {
            if progress.cancelled() {
                break;
            }
            progress.report(done as u64, total, &action.path.to_string_lossy());

            let written = match &action.shift {
                Some(delta) => writer.shift_dates(&action.path, delta),
                None => writer.write_dates(
                    &action.path,
                    &DateSet {
                        date: Some(action.new_date),
                    },
                ),
            };

            if let Err(e) = written {
                summary.failures.push((action.path.clone(), e.to_string()));
                continue;
            }

            summary.outcomes.push(verify(&action));
        }

        progress.report(total, total, "done");
        writer.close()?;
        Ok(Outcome { data: summary })
    }
}

/// Confirm what a write actually did by reading the file back.
fn verify(action: &DateRepairAction) -> DateRepairOutcome {
    let metadata_verified = read_meta(&action.path)
        .ok()
        .and_then(|m| m.capture)
        .map(|dt| dt == action.new_date)
        .unwrap_or(false);

    // exiftool writes FileModifyDate in local time; allow a day either side so a
    // timezone offset is not mistaken for a failed write.
    let filesystem_verified = modified_time(&action.path)
        .map(|actual| {
            let expected = action.new_date.and_utc().timestamp();
            (actual.and_utc().timestamp() - expected).abs() <= 86_400
        })
        .unwrap_or(false);

    let mut notes = Vec::new();
    if !metadata_verified {
        notes.push("metadata date did not read back as intended".to_string());
    }
    if !filesystem_verified {
        notes.push("filesystem modification time did not read back as intended".to_string());
    }
    if !birth_time_is_settable() {
        notes.push(
            "this platform has no settable creation time, so only the modification \
             time was changed"
                .to_string(),
        );
    }

    DateRepairOutcome {
        path: action.path.clone(),
        intended: action.new_date,
        metadata_verified,
        filesystem_verified,
        note: if notes.is_empty() {
            None
        } else {
            Some(notes.join("; "))
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dt(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y:%m:%d %H:%M:%S").unwrap()
    }

    #[test]
    fn every_extension_group_is_recognised() {
        for ext in IMAGE_EXTENSIONS
            .iter()
            .chain(RAW_EXTENSIONS.iter())
            .chain(VIDEO_EXTENSIONS.iter())
        {
            let lower = PathBuf::from(format!("f.{ext}"));
            let upper = PathBuf::from(format!("f.{}", ext.to_uppercase()));
            assert!(is_supported_media(&lower), "{ext} should be supported");
            assert!(is_supported_media(&upper), "{ext} uppercase");
        }
        assert!(!is_supported_media(Path::new("notes.txt")));
        assert!(!is_supported_media(Path::new("noextension")));
    }

    #[test]
    fn a_shift_delta_parses_and_applies() {
        let d = ShiftDelta::parse("+5:0:0 0:0:0").unwrap();
        assert!(!d.negative);
        assert_eq!(d.years, 5);
        assert_eq!(
            d.apply(dt("2019:01:02 03:04:05")),
            Some(dt("2024:01:02 03:04:05"))
        );
    }

    #[test]
    fn a_negative_shift_goes_backwards() {
        let d = ShiftDelta::parse("-0:1:0 0:0:0").unwrap();
        assert!(d.negative);
        assert_eq!(
            d.apply(dt("2024:03:15 10:00:00")),
            Some(dt("2024:02:15 10:00:00"))
        );
    }

    #[test]
    fn a_shift_delta_handles_the_time_half() {
        let d = ShiftDelta::parse("0:0:1 2:30:15").unwrap();
        assert_eq!(
            d.apply(dt("2024:01:01 00:00:00")),
            Some(dt("2024:01:02 02:30:15"))
        );
    }

    #[test]
    fn a_malformed_delta_is_rejected_rather_than_guessed() {
        assert!(ShiftDelta::parse("tomorrow").is_err());
        assert!(ShiftDelta::parse("1:2").is_err());
        assert!(ShiftDelta::parse("1:2:3 4:5:6 7:8:9").is_err());
        // A per-field sign is ambiguous and is refused.
        assert!(ShiftDelta::parse("1:-2:3 0:0:0").is_err());
    }

    #[test]
    fn the_magnitude_round_trips_through_exiftool_syntax() {
        let d = ShiftDelta::parse("+1:2:3 4:5:6").unwrap();
        assert_eq!(d.magnitude(), "1:2:3 4:5:6");
    }

    #[test]
    fn a_summary_is_only_fully_verified_when_everything_confirmed() {
        let mut s = DateRepairSummary::default();
        assert!(s.fully_verified(), "an empty run is vacuously verified");

        s.outcomes.push(DateRepairOutcome {
            path: PathBuf::from("a"),
            intended: dt("2024:01:01 00:00:00"),
            metadata_verified: true,
            filesystem_verified: false,
            note: None,
        });
        assert!(!s.fully_verified());
        assert_eq!(s.verified_count(), 0);

        s.outcomes[0].filesystem_verified = true;
        assert!(s.fully_verified());
        assert_eq!(s.verified_count(), 1);

        s.failures.push((PathBuf::from("b"), "boom".into()));
        assert!(!s.fully_verified());
    }

    /// The repair path used to treat a folder as a file: `read_meta` failed on
    /// it and the whole request came back "0 files would be redated, 1
    /// skipped" — the tool doing nothing on the most obvious input.
    #[test]
    fn a_folder_contributes_the_media_inside_it_to_a_repair() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("roll");
        std::fs::create_dir_all(folder.join("deeper")).unwrap();
        std::fs::write(folder.join("a.jpg"), b"x").unwrap();
        std::fs::write(folder.join("b.jpg"), b"y").unwrap();
        std::fs::write(folder.join("notes.txt"), b"z").unwrap();
        std::fs::write(folder.join("deeper/c.jpg"), b"w").unwrap();

        let when = chrono::NaiveDate::from_ymd_opt(2024, 5, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();

        let shallow = DateRepairTool
            .plan(&DateRepairParams {
                paths: vec![folder.clone()],
                mode: RepairMode::Manual(when),
                recursive: false,
            })
            .unwrap()
            .data;
        assert_eq!(
            shallow.actions.len(),
            2,
            "the two media files at the top, and not the .txt: {:?}",
            shallow.actions
        );

        let deep = DateRepairTool
            .plan(&DateRepairParams {
                paths: vec![folder],
                mode: RepairMode::Manual(when),
                recursive: true,
            })
            .unwrap()
            .data;
        assert_eq!(deep.actions.len(), 3, "recursive reaches the subfolder");
    }

    /// A folder with nothing readable says so rather than reporting success
    /// over an empty list.
    #[test]
    fn a_folder_with_no_media_is_reported_rather_than_silently_empty() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("empty");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("readme.txt"), b"x").unwrap();

        let plan = DateRepairTool
            .plan(&DateRepairParams {
                paths: vec![folder],
                mode: RepairMode::Auto,
                recursive: false,
            })
            .unwrap()
            .data;

        assert!(plan.actions.is_empty());
        assert_eq!(plan.skipped.len(), 1);
        assert!(
            plan.skipped[0].reason.contains("no media"),
            "{:?}",
            plan.skipped
        );
    }
}
