use chrono::{DateTime, NaiveDateTime};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(serde::Deserialize)]
struct TakeoutJson {
    #[serde(rename = "photoTakenTime")]
    photo_taken_time: Option<PhotoTakenTime>,
}

#[derive(serde::Deserialize)]
struct PhotoTakenTime {
    timestamp: String,
}

pub fn find_takeout_date(media_path: &Path) -> Option<NaiveDateTime> {
    let sidecar_path = find_sidecar(media_path)?;
    let content = fs::read_to_string(&sidecar_path).ok()?;
    let json: TakeoutJson = serde_json::from_str(&content).ok()?;

    if let Some(time) = json.photo_taken_time {
        if let Ok(ts) = time.timestamp.parse::<i64>() {
            if let Some(dt) = DateTime::from_timestamp(ts, 0) {
                return Some(dt.naive_utc());
            }
        }
    }
    None
}

fn find_sidecar(media_path: &Path) -> Option<PathBuf> {
    let filename = media_path.file_name()?.to_string_lossy().to_string();
    let dir = media_path.parent()?;

    // 1. Exact match: media.jpg -> media.jpg.json
    let exact = dir.join(format!("{}.json", filename));
    if exact.exists() {
        return Some(exact);
    }

    // 2. Truncation: Google limits base name to ~46 chars before .json
    let mut truncated = filename.clone();
    if truncated.len() > 46 {
        truncated.truncate(46);
    }
    let trunc_path = dir.join(format!("{}.json", truncated));
    if trunc_path.exists() {
        return Some(trunc_path);
    }

    // 3. (1) suffix on the media file e.g. media(1).jpg -> media.jpg(1).json
    if filename.contains("(1)") {
        let no_suffix = filename.replace("(1)", "");
        let suffix_json = dir.join(format!("{}.json", no_suffix));
        if suffix_json.exists() {
            return Some(suffix_json);
        }

        let file_stem = media_path.file_stem()?.to_string_lossy().to_string();
        let ext = media_path.extension()?.to_string_lossy().to_string();

        // media.jpg(1).json
        let weird_suffix = dir.join(format!("{}.{}(1).json", file_stem.replace("(1)", ""), ext));
        if weird_suffix.exists() {
            return Some(weird_suffix);
        }
    }

    None
}
