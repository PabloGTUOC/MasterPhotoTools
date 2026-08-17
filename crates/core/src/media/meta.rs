use crate::error::Error;
use chrono::NaiveDateTime;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum TagSource {
    DateTimeOriginal,
    CreateDate,
    QuickTimeCreationDate,
    QuickTimeCreateDate,
    KeysCreationDate,
    XmpCreateDate,
    QuickTimeModifyDate,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Orientation {
    Normal,
}

#[derive(Debug, Clone)]
pub struct MediaMeta {
    pub width: u32,
    pub height: u32,
    pub capture: Option<NaiveDateTime>,
    pub capture_source: Option<TagSource>,
    pub camera: Option<String>,
    pub orientation: Orientation,
}

pub fn read_meta(path: &Path) -> Result<MediaMeta, Error> {
    let mut parser = nom_exif::MediaParser::new();
    let ms = nom_exif::MediaSource::open(path).map_err(|e| Error::Internal(e.to_string()))?;

    let exif = match parser.parse_exif(ms) {
        Ok(e) => e,
        Err(_) => return Ok(empty_meta()),
    };

    let mut tags = HashMap::new();
    for tag in exif {
        if let Some(val) = tag.value() {
            tags.insert(format!("{}", tag.tag()), val.to_string());
        }
    }

    let (capture, capture_source) = get_best_date(&tags);
    let camera = tags.get("Model").map(|s| s.trim_matches('"').to_string());
    let width = tags
        .get("PixelXDimension")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let height = tags
        .get("PixelYDimension")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    Ok(MediaMeta {
        width,
        height,
        capture,
        capture_source,
        camera,
        orientation: Orientation::Normal, // Simplified
    })
}

fn empty_meta() -> MediaMeta {
    MediaMeta {
        width: 0,
        height: 0,
        capture: None,
        capture_source: None,
        camera: None,
        orientation: Orientation::Normal,
    }
}

fn parse_date(value: &str) -> Option<NaiveDateTime> {
    let value = value.trim_matches('"');
    if value == "0000:00:00 00:00:00" {
        return None;
    }
    // Remove timezone suffixes
    let clean = value
        .split('+')
        .next()
        .unwrap_or(value)
        .split('Z')
        .next()
        .unwrap_or(value)
        .trim();
    let clean = clean.replace('-', ":");
    NaiveDateTime::parse_from_str(&clean, "%Y:%m:%d %H:%M:%S").ok()
}

fn get_best_date(tags: &HashMap<String, String>) -> (Option<NaiveDateTime>, Option<TagSource>) {
    let order = [
        ("DateTimeOriginal", TagSource::DateTimeOriginal),
        ("CreateDate", TagSource::CreateDate),
        ("CreationDate", TagSource::QuickTimeCreationDate),
        ("KeysCreationDate", TagSource::KeysCreationDate),
        ("XmpCreateDate", TagSource::XmpCreateDate),
        ("ModifyDate", TagSource::QuickTimeModifyDate),
    ];
    for (tag, source) in order {
        if let Some(val) = tags.get(tag) {
            if let Some(dt) = parse_date(val) {
                return (Some(dt), Some(source));
            }
        }
    }
    (None, None)
}

#[derive(Default)]
pub struct DateSet {
    pub date: Option<NaiveDateTime>,
}

pub struct ExifWriter {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl ExifWriter {
    pub fn start() -> Result<Self, Error> {
        let mut child = Command::new("exiftool")
            .args(["-stay_open", "True", "-@", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| Error::Internal(format!("Failed to start exiftool: {}", e)))?;

        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());

        Ok(Self {
            child,
            stdin,
            stdout,
        })
    }

    pub fn write_dates(&mut self, path: &Path, set: &DateSet) -> Result<(), Error> {
        if let Some(date) = set.date {
            let date_str = date.format("%Y:%m:%d %H:%M:%S").to_string();
            writeln!(self.stdin, "-AllDates={}", date_str)?;
            writeln!(self.stdin, "-overwrite_original")?;
            writeln!(self.stdin, "{}", path.display())?;
            writeln!(self.stdin, "-execute")?;
            self.stdin.flush()?;

            self.wait_for_ready()?;
        }
        Ok(())
    }

    pub fn shift_dates(&mut self, _path: &Path, _delta: &str) -> Result<(), Error> {
        todo!("Shift dates logic")
    }

    pub fn copy_metadata(&mut self, src: &Path, dst: &Path) -> Result<(), Error> {
        writeln!(self.stdin, "-TagsFromFile")?;
        writeln!(self.stdin, "{}", src.display())?;
        writeln!(self.stdin, "-all:all")?;
        writeln!(self.stdin, "-overwrite_original")?;
        writeln!(self.stdin, "{}", dst.display())?;
        writeln!(self.stdin, "-execute")?;
        self.stdin.flush()?;

        self.wait_for_ready()?;
        Ok(())
    }

    fn wait_for_ready(&mut self) -> Result<(), Error> {
        let mut line = String::new();
        loop {
            line.clear();
            let bytes = self.stdout.read_line(&mut line)?;
            if bytes == 0 {
                return Err(Error::Internal("exiftool closed unexpectedly".into()));
            }
            if line.contains("{ready}") {
                break;
            }
        }
        Ok(())
    }
}

impl Drop for ExifWriter {
    fn drop(&mut self) {
        let _ = writeln!(self.stdin, "-stay_open\nFalse");
        let _ = self.child.wait();
    }
}
