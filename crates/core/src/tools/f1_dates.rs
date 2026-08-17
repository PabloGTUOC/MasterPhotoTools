use crate::error::Error;
use crate::jobs::{Outcome, Progress, ToolResult};
use crate::media::meta::{read_meta, DateSet, ExifWriter, MediaMeta};
use crate::tools::{Plan, Skip, Tool};
use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use std::fs;
use std::path::{Path, PathBuf};

#[cfg(any(target_os = "macos", target_os = "freebsd", target_os = "openbsd"))]
fn get_creation_time(metadata: &fs::Metadata) -> Option<std::time::SystemTime> {
    metadata.created().ok()
}

#[cfg(not(any(target_os = "macos", target_os = "freebsd", target_os = "openbsd")))]
fn get_creation_time(metadata: &fs::Metadata) -> Option<std::time::SystemTime> {
    metadata.modified().ok()
}

pub fn get_fs_time(path: &Path) -> Option<NaiveDateTime> {
    if let Ok(metadata) = fs::metadata(path) {
        if let Some(st) = get_creation_time(&metadata) {
            let dt: DateTime<Utc> = st.into();
            return Some(dt.naive_utc());
        }
    }
    None
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DateStatus {
    Ok,
    Mismatch,
    MissingMetadata,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScanResult {
    pub name: String,
    pub path: PathBuf,
    pub metadata_date: Option<NaiveDateTime>,
    pub tag_source: Option<String>,
    pub fs_date: Option<NaiveDateTime>,
    pub status: DateStatus,
}

pub fn scan_dates(path: &Path, recursive: bool) -> Result<Vec<ScanResult>, Error> {
    let mut results = Vec::new();
    let mut dirs_to_visit = vec![path.to_path_buf()];

    while let Some(dir) = dirs_to_visit.pop() {
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if recursive {
                        dirs_to_visit.push(path);
                    }
                } else {
                    if is_supported_media(&path) {
                        let meta = read_meta(&path).unwrap_or(MediaMeta {
                            width: 0,
                            height: 0,
                            capture: None,
                            capture_source: None,
                            camera: None,
                            orientation: crate::media::meta::Orientation::Normal,
                        });

                        let fs_date = get_fs_time(&path);

                        let status = match (meta.capture, fs_date) {
                            (None, _) => DateStatus::MissingMetadata,
                            (Some(m), Some(f)) => {
                                let diff =
                                    (m.and_utc().timestamp() - f.and_utc().timestamp()).abs();
                                if diff > 60 {
                                    DateStatus::Mismatch
                                } else {
                                    DateStatus::Ok
                                }
                            }
                            (Some(_), None) => DateStatus::Mismatch,
                        };

                        results.push(ScanResult {
                            name: path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string(),
                            path,
                            metadata_date: meta.capture,
                            tag_source: meta.capture_source.map(|s| format!("{:?}", s)),
                            fs_date,
                            status,
                        });
                    }
                }
            }
        }
    }

    Ok(results)
}

fn is_supported_media(path: &Path) -> bool {
    let exts = [
        "jpg", "jpeg", "png", "gif", "tif", "tiff", "heic", "heif", "dng", "nef", "arw", "cr2",
        "raf", "mov", "mp4", "m4v", "mts", "m2ts", "3gp", "avi",
    ];
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        exts.contains(&ext.to_lowercase().as_str())
    } else {
        false
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum RepairMode {
    Auto,
    Manual(NaiveDateTime),
    // Using i64 seconds instead of chrono::Duration to make it serde friendly trivially
    Shift(i64),
    Sidecar,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct DateRepairParams {
    pub paths: Vec<PathBuf>,
    pub mode: RepairMode,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct DateRepairAction {
    pub path: PathBuf,
    pub new_date: NaiveDateTime,
}

pub struct DateRepairTool;

impl Tool for DateRepairTool {
    type Params = DateRepairParams;
    type Action = DateRepairAction;
    type Summary = ();

    fn plan(&self, p: &Self::Params) -> ToolResult<Plan<Self::Action>> {
        let mut actions = Vec::new();
        let mut skipped = Vec::new();

        for path in &p.paths {
            if !path.exists() {
                skipped.push(Skip {
                    file: path.to_string_lossy().to_string(),
                    reason: "File not found".into(),
                });
                continue;
            }

            let new_date = match &p.mode {
                RepairMode::Auto => {
                    let meta = read_meta(path).ok();
                    if let Some(m) = meta.and_then(|m| m.capture) {
                        m
                    } else {
                        skipped.push(Skip {
                            file: path.to_string_lossy().to_string(),
                            reason: "No metadata date for auto mode".into(),
                        });
                        continue;
                    }
                }
                RepairMode::Manual(dt) => *dt,
                RepairMode::Shift(secs) => {
                    let meta = read_meta(path).ok();
                    if let Some(m) = meta.and_then(|m| m.capture) {
                        m + Duration::seconds(*secs)
                    } else {
                        skipped.push(Skip {
                            file: path.to_string_lossy().to_string(),
                            reason: "No metadata date for shift mode".into(),
                        });
                        continue;
                    }
                }
                RepairMode::Sidecar => {
                    if let Some(dt) = crate::tools::f2_takeout::find_takeout_date(path) {
                        dt
                    } else {
                        skipped.push(Skip {
                            file: path.to_string_lossy().to_string(),
                            reason: "No sidecar found".into(),
                        });
                        continue;
                    }
                }
            };

            actions.push(DateRepairAction {
                path: path.clone(),
                new_date,
            });
        }

        Ok(Outcome {
            data: Plan { actions, skipped },
        })
    }

    fn apply(
        &self,
        plan: Plan<Self::Action>,
        _progress: &dyn Progress,
    ) -> ToolResult<Self::Summary> {
        let mut writer = ExifWriter::start()?;
        for action in plan.actions {
            let set = DateSet {
                date: Some(action.new_date),
            };
            writer.write_dates(&action.path, &set)?;
        }
        Ok(Outcome { data: () })
    }
}
