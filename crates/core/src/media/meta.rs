//! In-process metadata reading (G3) and the persistent `exiftool` writer (G4).

use crate::error::Error;
use chrono::NaiveDateTime;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread::JoinHandle;
use std::time::Duration;

/// Which tag supplied a capture date.
///
/// The variants are declared in the preference order of specification F1 and
/// [`TagSource::PREFERENCE_ORDER`] depends on that order.
#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy, PartialOrd, Ord)]
pub enum TagSource {
    /// `EXIF:DateTimeOriginal`
    ExifDateTimeOriginal,
    /// `EXIF:CreateDate`
    ExifCreateDate,
    /// `QuickTime:CreationDate`
    QuickTimeCreationDate,
    /// `QuickTime:CreateDate`
    QuickTimeCreateDate,
    /// `Keys:CreationDate`
    KeysCreationDate,
    /// `XMP:CreateDate`
    XmpCreateDate,
    /// `QuickTime:ModifyDate`
    QuickTimeModifyDate,
}

impl TagSource {
    /// Specification F1's preference order, first hit wins.
    pub const PREFERENCE_ORDER: [TagSource; 7] = [
        TagSource::ExifDateTimeOriginal,
        TagSource::ExifCreateDate,
        TagSource::QuickTimeCreationDate,
        TagSource::QuickTimeCreateDate,
        TagSource::KeysCreationDate,
        TagSource::XmpCreateDate,
        TagSource::QuickTimeModifyDate,
    ];

    /// The tag's name as exiftool spells it, for display and reporting.
    pub fn name(&self) -> &'static str {
        match self {
            TagSource::ExifDateTimeOriginal => "EXIF:DateTimeOriginal",
            TagSource::ExifCreateDate => "EXIF:CreateDate",
            TagSource::QuickTimeCreationDate => "QuickTime:CreationDate",
            TagSource::QuickTimeCreateDate => "QuickTime:CreateDate",
            TagSource::KeysCreationDate => "Keys:CreationDate",
            TagSource::XmpCreateDate => "XMP:CreateDate",
            TagSource::QuickTimeModifyDate => "QuickTime:ModifyDate",
        }
    }

    /// True for tags whose timestamps are stored in UTC. Reading these as local
    /// time double-shifts them (specification F1).
    pub fn is_utc(&self) -> bool {
        matches!(
            self,
            TagSource::QuickTimeCreationDate
                | TagSource::QuickTimeCreateDate
                | TagSource::KeysCreationDate
                | TagSource::QuickTimeModifyDate
        )
    }
}

impl std::fmt::Display for TagSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// EXIF orientation (tag 0x0112), all eight defined values.
#[derive(Debug, PartialEq, Eq, Clone, Copy, Default)]
pub enum Orientation {
    #[default]
    Normal = 1,
    FlipHorizontal = 2,
    Rotate180 = 3,
    FlipVertical = 4,
    Transpose = 5,
    Rotate90 = 6,
    Transverse = 7,
    Rotate270 = 8,
}

impl Orientation {
    pub fn from_exif(value: u16) -> Self {
        match value {
            2 => Orientation::FlipHorizontal,
            3 => Orientation::Rotate180,
            4 => Orientation::FlipVertical,
            5 => Orientation::Transpose,
            6 => Orientation::Rotate90,
            7 => Orientation::Transverse,
            8 => Orientation::Rotate270,
            _ => Orientation::Normal,
        }
    }

    /// True when applying this orientation swaps width and height.
    pub fn swaps_axes(&self) -> bool {
        matches!(
            self,
            Orientation::Transpose
                | Orientation::Rotate90
                | Orientation::Transverse
                | Orientation::Rotate270
        )
    }
}

/// Where a file says it was taken.
///
/// Read so the geotagging tool can tell a photograph that already knows where
/// it was from one that does not — writing over the first would replace a
/// measurement taken at the time with one inferred afterwards.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GpsFix {
    /// Signed decimal degrees, positive north.
    pub lat: f64,
    /// Signed decimal degrees, positive east.
    pub lon: f64,
    /// Signed metres. `None` where the file carries no altitude, which is not
    /// the same as an altitude of zero.
    pub altitude: Option<f64>,
}

/// `Eq` is deliberately absent: coordinates are floats, and two fixes being
/// "the same" is a question about tolerance, not about bit patterns. The
/// tolerance lives with the tool that has to answer it
/// (`tools::geotag::same_position`).
#[derive(Debug, Clone, PartialEq)]
pub struct MediaMeta {
    pub width: u32,
    pub height: u32,
    pub capture: Option<NaiveDateTime>,
    pub capture_source: Option<TagSource>,
    pub camera: Option<String>,
    pub orientation: Orientation,
    /// Where the file says it was taken, if it says.
    pub gps: Option<GpsFix>,
    /// The UTC offset the camera recorded alongside the capture time, in
    /// minutes east of Greenwich.
    ///
    /// EXIF capture times are local wall-clock with no zone, which is why
    /// `capture` is naive. `OffsetTimeOriginal` is the one tag that closes that
    /// gap, and where a file carries it there is nothing to guess: it is the
    /// difference between joining a photograph to a track and asking somebody
    /// which hour they think they were in. Phones write it; most cameras do
    /// not.
    pub utc_offset_minutes: Option<i32>,
}

impl MediaMeta {
    pub fn empty() -> Self {
        Self {
            width: 0,
            height: 0,
            capture: None,
            capture_source: None,
            camera: None,
            orientation: Orientation::Normal,
            gps: None,
            utc_offset_minutes: None,
        }
    }
}

/// Parse an EXIF offset — `+02:00`, `-05:00`, `+05:45` — into minutes east.
///
/// Returned in minutes rather than hours because three populated time zones
/// are not on a whole hour, and a reader that rounded them would put every
/// photograph taken in one of them half an hour's travel from where it was.
pub fn parse_utc_offset(raw: &str) -> Option<i32> {
    let raw = raw.trim().trim_matches('"').trim();
    let (sign, body) = match raw.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, raw.strip_prefix('+').unwrap_or(raw)),
    };
    let (hours, minutes) = body.split_once(':')?;
    let hours: i32 = hours.trim().parse().ok()?;
    let minutes: i32 = minutes.trim().parse().ok()?;
    if !(0..=14).contains(&hours) || !(0..60).contains(&minutes) {
        return None;
    }
    Some(sign * (hours * 60 + minutes))
}

/// The sentinel exiftool writes for "no date". Counts as absent (F1).
const ABSENT_SENTINEL: &str = "0000:00:00 00:00:00";

/// Normalise a raw tag string to `YYYY:MM:DD HH:MM:SS`.
///
/// Handles, per specification F1:
/// - the `0000:00:00 00:00:00` sentinel, which is absent rather than a date,
/// - timezone suffixes (`Z`, `+01:00`, `-05:00`), which are dropped,
/// - `YYYY-MM-DD` input, which is accepted and converted,
/// - an ISO `T` between date and time.
pub fn normalise_datetime(raw: &str) -> Option<NaiveDateTime> {
    let value = raw.trim().trim_matches('"').trim();
    if value.is_empty() || value.starts_with("0000:00:00") || value == ABSENT_SENTINEL {
        return None;
    }

    // Strip a timezone suffix. Only look past the date portion, so the hyphens
    // in a `YYYY-MM-DD` date are never mistaken for a negative UTC offset.
    let scan_from = value.len().min(10);
    let mut body = value;
    if let Some(rel) = value[scan_from..].find(['Z', 'z', '+']) {
        body = &value[..scan_from + rel];
    }
    if let Some(rel) = value[scan_from..].find('-') {
        let cut = scan_from + rel;
        if cut < body.len() {
            body = &value[..cut];
        }
    }
    let body = body.trim();

    // Normalise the date separators without touching the time's colons.
    let (date_part, rest) = match body.find([' ', 'T']) {
        Some(i) => (&body[..i], &body[i + 1..]),
        None => (body, ""),
    };
    let date_part = date_part.replace('-', ":");

    if rest.is_empty() {
        // A date with no time is midnight.
        return NaiveDateTime::parse_from_str(
            &format!("{date_part} 00:00:00"),
            "%Y:%m:%d %H:%M:%S",
        )
        .ok();
    }

    // Sub-second precision is not part of the normalised form.
    let time_part = rest.split('.').next().unwrap_or(rest).trim();
    NaiveDateTime::parse_from_str(&format!("{date_part} {time_part}"), "%Y:%m:%d %H:%M:%S").ok()
}

/// Resolve a capture date from candidate tags using F1's preference order.
///
/// Pure over its input so every position in the order is directly testable,
/// including the tags no current file format exposes to us.
pub fn best_date(
    candidates: &HashMap<TagSource, String>,
) -> (Option<NaiveDateTime>, Option<TagSource>) {
    for source in TagSource::PREFERENCE_ORDER {
        if let Some(raw) = candidates.get(&source) {
            if let Some(dt) = normalise_datetime(raw) {
                return (Some(dt), Some(source));
            }
        }
    }
    (None, None)
}

const VIDEO_EXTENSIONS: [&str; 7] = ["mov", "mp4", "m4v", "mts", "m2ts", "3gp", "avi"];

pub fn is_video(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| VIDEO_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Read metadata in-process (G3).
///
/// Never invokes an external program: specification §9.1 rule 1 makes this the
/// difference between a two-second card scan and a two-minute one.
///
/// A file whose metadata cannot be parsed yields an empty [`MediaMeta`] rather
/// than an error — one unreadable frame must not abort a 400-frame scan.
pub fn read_meta(path: &Path) -> Result<MediaMeta, Error> {
    if is_video(path) {
        return Ok(read_video_meta(path));
    }
    Ok(read_image_meta(path))
}

/// Parse metadata with the whole file in memory.
///
/// The fallback for a file the streaming reader opens but cannot walk. It costs
/// the file's size in memory — 103 MB for a camera TIFF — which is why it is
/// only reached after the streaming attempt has failed.
fn read_image_meta_from_memory(path: &Path) -> MediaMeta {
    use nom_exif::{MediaParser, MediaSource};

    let Ok(bytes) = std::fs::read(path) else {
        return MediaMeta::empty();
    };
    // `from_memory` with the unified `parse_exif`, not the deprecated
    // `from_bytes` / `parse_exif_from_bytes` pair the crate is removing in v4.
    let Ok(source) = MediaSource::from_memory(bytes) else {
        return MediaMeta::empty();
    };
    let mut parser = MediaParser::new();
    match parser.parse_exif(source) {
        Ok(iter) => collect_image(iter),
        Err(_) => MediaMeta::empty(),
    }
}

fn read_image_meta(path: &Path) -> MediaMeta {
    use nom_exif::{MediaParser, MediaSource};

    let mut parser = MediaParser::new();
    let source = match MediaSource::open(path) {
        Ok(s) => s,
        Err(_) => return MediaMeta::empty(),
    };

    // The streaming reader cannot parse every file it can open. A camera TIFF
    // fails part-way through its IFD — `Incomplete(Size(..))`, nom asking for
    // bytes the incremental source did not hand it — while the same file parses
    // correctly from memory. So a failure here is retried against the whole
    // file rather than reported as "no metadata": the dates are demonstrably
    // there, and a silent empty answer sent every one of them to the skipped
    // list as "No metadata date to copy".
    //
    // Retried, not taken first: reading is the common path, this costs the
    // file's size in memory, and §9.1's card scan budget is written against the
    // streaming reader.
    let iter = match parser.parse_exif(source) {
        Ok(i) => i,
        Err(_) => return read_image_meta_from_memory(path),
    };

    collect_image(iter)
}

/// Everything an image's EXIF says, from the one iterator.
///
/// The GPS block is asked for **before** the entries are walked, because
/// walking consumes the iterator and the block is a sub-IFD the walk flattens.
/// It is read here rather than inside `collect_exif` so that function stays a
/// pure fold over entries, and its tests keep working from a list of tags with
/// no file behind them.
fn collect_image(iter: nom_exif::ExifIter) -> MediaMeta {
    let gps = iter.parse_gps().ok().flatten().and_then(to_fix);

    let mut meta = collect_exif(iter);
    meta.gps = gps;
    meta
}

/// A parsed GPS block, if it holds a position at all.
///
/// A block with a hemisphere and no coordinate is not a position, and reporting
/// it as one would tell the geotagging tool to leave a photograph alone that
/// has nothing to leave alone.
fn to_fix(info: nom_exif::GPSInfo) -> Option<GpsFix> {
    Some(GpsFix {
        lat: info.latitude_decimal()?,
        lon: info.longitude_decimal()?,
        altitude: info.altitude_meters(),
    })
}

/// Walk an EXIF iterator into a [`MediaMeta`].
///
/// Shared by the streaming and in-memory paths so the two cannot disagree
/// about which tag wins — F1's preference order is the whole point of this.
fn collect_exif(iter: impl IntoIterator<Item = nom_exif::ExifIterEntry>) -> MediaMeta {
    use nom_exif::ExifTag;

    let mut candidates: HashMap<TagSource, String> = HashMap::new();
    let mut meta = MediaMeta::empty();
    let mut fallback_width = 0u32;
    let mut fallback_height = 0u32;
    // Which offset tag the current value came from; see the match arm below.
    let mut offset_rank = -1i8;

    for entry in iter {
        let Some(tag) = entry.tag().tag() else {
            continue;
        };
        let Some(value) = entry.value() else { continue };

        match tag {
            ExifTag::DateTimeOriginal => {
                insert_date(&mut candidates, TagSource::ExifDateTimeOriginal, value);
            }
            ExifTag::CreateDate => {
                insert_date(&mut candidates, TagSource::ExifCreateDate, value);
            }
            ExifTag::Model => {
                meta.camera = Some(value.to_string().trim_matches('"').trim().to_string());
            }
            // Three tags can carry the offset, and they are not interchangeable:
            // `OffsetTimeOriginal` belongs to the moment the shutter opened,
            // which is the moment a track is joined on. The others are taken
            // only when it is absent, and never over it — which is why this
            // records the tag as well as the value.
            ExifTag::OffsetTimeOriginal | ExifTag::OffsetTimeDigitized | ExifTag::OffsetTime => {
                let rank = match tag {
                    ExifTag::OffsetTimeOriginal => 2,
                    ExifTag::OffsetTimeDigitized => 1,
                    _ => 0,
                };
                if let Some(minutes) = parse_utc_offset(&value.to_string()) {
                    if rank >= offset_rank {
                        offset_rank = rank;
                        meta.utc_offset_minutes = Some(minutes);
                    }
                }
            }
            ExifTag::Orientation => {
                if let Some(v) = as_u32(value) {
                    meta.orientation = Orientation::from_exif(v as u16);
                }
            }
            ExifTag::ExifImageWidth => meta.width = as_u32(value).unwrap_or(0),
            ExifTag::ExifImageHeight => meta.height = as_u32(value).unwrap_or(0),
            ExifTag::ImageWidth => fallback_width = as_u32(value).unwrap_or(0),
            ExifTag::ImageHeight => fallback_height = as_u32(value).unwrap_or(0),
            _ => {}
        }
    }

    // `ExifImageWidth` is the authority; IFD0's `ImageWidth` is the fallback for
    // files that only carry the latter (TIFF, many RAW containers).
    if meta.width == 0 {
        meta.width = fallback_width;
    }
    if meta.height == 0 {
        meta.height = fallback_height;
    }

    let (capture, source) = best_date(&candidates);
    meta.capture = capture;
    meta.capture_source = source;
    meta
}

fn read_video_meta(path: &Path) -> MediaMeta {
    use nom_exif::{MediaParser, MediaSource, TrackInfoTag};

    let mut parser = MediaParser::new();
    let source = match MediaSource::open(path) {
        Ok(s) => s,
        Err(_) => return MediaMeta::empty(),
    };
    let track = match parser.parse_track(source) {
        Ok(t) => t,
        Err(_) => return MediaMeta::empty(),
    };

    let mut candidates: HashMap<TagSource, String> = HashMap::new();
    let mut meta = MediaMeta::empty();

    if let Some(value) = track.get(TrackInfoTag::CreateDate) {
        insert_date(&mut candidates, TagSource::QuickTimeCreateDate, value);
    }
    if let Some(value) = track.get(TrackInfoTag::Model) {
        meta.camera = Some(value.to_string().trim_matches('"').trim().to_string());
    }
    if let Some(v) = track.get(TrackInfoTag::Width).and_then(as_u32) {
        meta.width = v;
    }
    if let Some(v) = track.get(TrackInfoTag::Height).and_then(as_u32) {
        meta.height = v;
    }

    // A phone films with the GPS on, so a video knows where it was even though
    // this tool will not write to one. The inventory says what a file carries;
    // it does not get to be silent about a thing it can see.
    meta.gps = track.gps_info().cloned().and_then(to_fix);

    let (capture, source) = best_date(&candidates);
    meta.capture = capture;
    meta.capture_source = source;
    meta
}

/// Render a parsed value into the normalised textual form the resolver expects.
///
/// QuickTime timestamps arrive as an offset-aware datetime and are taken as UTC,
/// which is what prevents the double-shift specification F1 warns about.
fn insert_date(
    candidates: &mut HashMap<TagSource, String>,
    source: TagSource,
    value: &nom_exif::EntryValue,
) {
    use nom_exif::EntryValue;
    let text = match value {
        EntryValue::NaiveDateTime(dt) => dt.format("%Y:%m:%d %H:%M:%S").to_string(),
        EntryValue::DateTime(dt) => {
            if source.is_utc() {
                dt.naive_utc().format("%Y:%m:%d %H:%M:%S").to_string()
            } else {
                dt.naive_local().format("%Y:%m:%d %H:%M:%S").to_string()
            }
        }
        other => other.to_string(),
    };
    candidates.insert(source, text);
}

fn as_u32(value: &nom_exif::EntryValue) -> Option<u32> {
    use nom_exif::EntryValue;
    match value {
        EntryValue::U8(v) => Some(*v as u32),
        EntryValue::U16(v) => Some(*v as u32),
        EntryValue::U32(v) => Some(*v),
        EntryValue::U64(v) => Some(*v as u32),
        EntryValue::I32(v) if *v >= 0 => Some(*v as u32),
        EntryValue::Text(t) => t.trim().parse().ok(),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// The persistent exiftool driver
// ---------------------------------------------------------------------------

/// Dates to write to a file.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DateSet {
    pub date: Option<NaiveDateTime>,
}

/// How long to wait for a single `-execute` to come back before giving up.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// A single long-lived `exiftool -stay_open` process (G4).
///
/// Starting one process per file costs 150–250 ms each regardless of file size,
/// which would add over a minute of pure overhead to a 500-file operation
/// (specification §2.6). One writer serves a whole batch.
///
/// Not `Sync`: it owns a pipe with request/response framing, so concurrent use
/// would interleave commands. Parallel callers should do their CPU work in
/// parallel and funnel metadata writes through one writer.
pub struct ExifWriter {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<std::io::Result<String>>,
    reader: Option<JoinHandle<()>>,
    timeout: Duration,
}

impl ExifWriter {
    pub fn start() -> Result<Self, Error> {
        Self::start_with_timeout(DEFAULT_TIMEOUT)
    }

    pub fn start_with_timeout(timeout: Duration) -> Result<Self, Error> {
        Self::start_with("exiftool", timeout)
    }

    /// Start against a specific program.
    ///
    /// Exists so tests can point at a shim that records how many processes were
    /// spawned — the G4 guarantee is otherwise only observable from outside.
    pub fn start_with(program: &str, timeout: Duration) -> Result<Self, Error> {
        let mut child = Command::new(program)
            .args(["-stay_open", "True", "-@", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| {
                Error::Internal(format!(
                    "Failed to start {program}: {e}. exiftool is required for metadata \
                     writing (specification §2.6) and must be on PATH."
                ))
            })?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| Error::Internal("exiftool stdin unavailable".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::Internal("exiftool stdout unavailable".into()))?;

        // Read on a thread so a hung child surfaces as a timeout rather than a
        // blocked caller.
        let (tx, lines) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            let buf = BufReader::new(stdout);
            for line in buf.lines() {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });

        let mut writer = Self {
            child,
            stdin,
            lines,
            reader: Some(reader),
            timeout,
        };

        // Handshake: prove the process is alive and framing works before any
        // caller depends on it.
        writer.execute(&["-ver".to_string()])?;
        Ok(writer)
    }

    /// Send one command block and wait for its `{ready}` sentinel.
    fn execute(&mut self, args: &[String]) -> Result<(), Error> {
        for arg in args {
            writeln!(self.stdin, "{arg}")?;
        }
        writeln!(self.stdin, "-execute")?;
        self.stdin.flush()?;
        self.wait_for_ready()
    }

    fn wait_for_ready(&mut self) -> Result<(), Error> {
        loop {
            match self.lines.recv_timeout(self.timeout) {
                Ok(Ok(line)) => {
                    if line.contains("{ready}") {
                        return Ok(());
                    }
                }
                Ok(Err(e)) => return Err(Error::Internal(format!("exiftool read failed: {e}"))),
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(Error::Internal("exiftool closed unexpectedly".into()));
                }
                Err(RecvTimeoutError::Timeout) => {
                    let _ = self.child.kill();
                    return Err(Error::Internal(format!(
                        "exiftool did not respond within {:?}",
                        self.timeout
                    )));
                }
            }
        }
    }

    /// Write the full date tag set for a file (specification F1).
    ///
    /// Images receive `DateTimeOriginal`, `CreateDate`, `ModifyDate` and
    /// `AllDates`; video receives `CreateDate`, `ModifyDate`, `MediaCreateDate`
    /// and `TrackCreateDate`. Both additionally receive `FileCreateDate` and
    /// `FileModifyDate`.
    pub fn write_dates(&mut self, path: &Path, set: &DateSet) -> Result<(), Error> {
        let Some(date) = set.date else {
            return Ok(());
        };
        let stamp = date.format("%Y:%m:%d %H:%M:%S").to_string();

        let mut args: Vec<String> = Vec::new();
        if is_video(path) {
            args.push(format!("-CreateDate={stamp}"));
            args.push(format!("-ModifyDate={stamp}"));
            args.push(format!("-MediaCreateDate={stamp}"));
            args.push(format!("-TrackCreateDate={stamp}"));
        } else {
            args.push(format!("-DateTimeOriginal={stamp}"));
            args.push(format!("-CreateDate={stamp}"));
            args.push(format!("-ModifyDate={stamp}"));
            args.push(format!("-AllDates={stamp}"));
        }
        args.push(format!("-FileCreateDate={stamp}"));
        args.push(format!("-FileModifyDate={stamp}"));
        args.push("-overwrite_original".to_string());
        args.push(path.display().to_string());

        self.execute(&args)
    }

    /// Shift every date by a delta, e.g. `+1:0:0 0:0:0` for a camera clock that
    /// was a year behind (specification F1 `shift` mode).
    ///
    /// The delta is exiftool's shift syntax: `Y:M:D h:m:s`, leading `+` or `-`.
    pub fn shift_dates(&mut self, path: &Path, delta: &str) -> Result<(), Error> {
        let delta = delta.trim();
        let (op, magnitude) = match delta.strip_prefix('-') {
            Some(rest) => ("-=", rest.trim()),
            None => ("+=", delta.trim_start_matches('+').trim()),
        };

        let mut args: Vec<String> = Vec::new();
        if is_video(path) {
            for tag in [
                "CreateDate",
                "ModifyDate",
                "MediaCreateDate",
                "TrackCreateDate",
            ] {
                args.push(format!("-{tag}{op}{magnitude}"));
            }
        } else {
            args.push(format!("-AllDates{op}{magnitude}"));
        }
        args.push(format!("-FileModifyDate{op}{magnitude}"));
        args.push("-overwrite_original".to_string());
        args.push(path.display().to_string());

        self.execute(&args)
    }

    /// Copy every tag from `src` into `dst`.
    pub fn copy_metadata(&mut self, src: &Path, dst: &Path) -> Result<(), Error> {
        self.execute(&[
            "-TagsFromFile".to_string(),
            src.display().to_string(),
            "-all:all".to_string(),
            "-overwrite_original".to_string(),
            dst.display().to_string(),
        ])
    }

    /// Set a single tag to a literal value.
    pub fn set_tag(&mut self, path: &Path, tag: &str, value: &str) -> Result<(), Error> {
        self.execute(&[
            format!("-{tag}={value}"),
            "-overwrite_original".to_string(),
            path.display().to_string(),
        ])
    }

    /// Apply several tag assignments to one file in a single `-execute`.
    ///
    /// The assignments arrive already rendered (`-GPSLatitude=52.5315490`),
    /// because what a tag *means* belongs to the tool that decided to write it,
    /// not to the process driver. Writing a position is nine tags, and nine
    /// round trips through the driver for one file would give up most of what
    /// keeping the process alive was for (G4).
    pub fn set_tags(&mut self, path: &Path, assignments: &[String]) -> Result<(), Error> {
        if assignments.is_empty() {
            return Ok(());
        }
        let mut args: Vec<String> = assignments.to_vec();
        args.push("-overwrite_original".to_string());
        args.push(path.display().to_string());
        self.execute(&args)
    }

    /// Shut the process down cleanly and wait for it to exit.
    pub fn close(mut self) -> Result<(), Error> {
        self.shutdown();
        Ok(())
    }

    fn shutdown(&mut self) {
        let _ = writeln!(self.stdin, "-stay_open");
        let _ = writeln!(self.stdin, "False");
        let _ = self.stdin.flush();
        let _ = self.child.wait();
        if let Some(handle) = self.reader.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for ExifWriter {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dt(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y:%m:%d %H:%M:%S").unwrap()
    }

    #[test]
    fn the_absent_sentinel_is_not_a_date() {
        assert_eq!(normalise_datetime("0000:00:00 00:00:00"), None);
        assert_eq!(normalise_datetime("0000:00:00"), None);
        assert_eq!(normalise_datetime(""), None);
        assert_eq!(normalise_datetime("   "), None);
    }

    #[test]
    fn timezone_suffixes_are_dropped() {
        let expected = dt("2024:05:01 12:00:00");
        assert_eq!(normalise_datetime("2024:05:01 12:00:00Z"), Some(expected));
        assert_eq!(
            normalise_datetime("2024:05:01 12:00:00+01:00"),
            Some(expected)
        );
        assert_eq!(
            normalise_datetime("2024:05:01 12:00:00-05:00"),
            Some(expected)
        );
    }

    #[test]
    fn hyphenated_dates_are_accepted_and_converted() {
        let expected = dt("2024:05:01 12:00:00");
        assert_eq!(normalise_datetime("2024-05-01 12:00:00"), Some(expected));
        assert_eq!(normalise_datetime("2024-05-01T12:00:00"), Some(expected));
        // A hyphenated date *and* a negative offset — the date's hyphens must
        // not be mistaken for the offset.
        assert_eq!(
            normalise_datetime("2024-05-01T12:00:00-05:00"),
            Some(expected)
        );
    }

    #[test]
    fn a_bare_date_is_midnight() {
        assert_eq!(
            normalise_datetime("2024:05:01"),
            Some(dt("2024:05:01 00:00:00"))
        );
    }

    #[test]
    fn sub_second_precision_is_discarded() {
        assert_eq!(
            normalise_datetime("2024:05:01 12:00:00.123"),
            Some(dt("2024:05:01 12:00:00"))
        );
    }

    #[test]
    fn nonsense_is_rejected() {
        assert_eq!(normalise_datetime("not a date"), None);
        assert_eq!(normalise_datetime("2024:13:45 99:99:99"), None);
    }

    /// Acceptance: for every position in F1's order, a file whose highest
    /// available tag is that one resolves to it.
    #[test]
    fn each_position_in_the_preference_order_wins_when_it_is_highest() {
        let stamps = [
            "2001:01:01 01:01:01",
            "2002:02:02 02:02:02",
            "2003:03:03 03:03:03",
            "2004:04:04 04:04:04",
            "2005:05:05 05:05:05",
            "2006:06:06 06:06:06",
            "2007:07:07 07:07:07",
        ];

        for skip in 0..TagSource::PREFERENCE_ORDER.len() {
            // Populate every tag from `skip` downwards; the one at `skip` is now
            // the highest available and must win.
            let mut candidates = HashMap::new();
            for (i, source) in TagSource::PREFERENCE_ORDER.iter().enumerate().skip(skip) {
                candidates.insert(*source, stamps[i].to_string());
            }

            let (date, source) = best_date(&candidates);
            assert_eq!(
                source,
                Some(TagSource::PREFERENCE_ORDER[skip]),
                "with tags from position {skip} down, that tag should win"
            );
            assert_eq!(date, Some(dt(stamps[skip])));
        }
    }

    #[test]
    fn a_tag_holding_the_sentinel_is_skipped_for_the_next_one() {
        let mut candidates = HashMap::new();
        candidates.insert(TagSource::ExifDateTimeOriginal, ABSENT_SENTINEL.to_string());
        candidates.insert(TagSource::ExifCreateDate, "2024:05:01 12:00:00".to_string());

        let (date, source) = best_date(&candidates);
        assert_eq!(source, Some(TagSource::ExifCreateDate));
        assert_eq!(date, Some(dt("2024:05:01 12:00:00")));
    }

    #[test]
    fn no_candidates_means_no_date() {
        assert_eq!(best_date(&HashMap::new()), (None, None));
    }

    #[test]
    fn quicktime_tags_are_utc_and_exif_tags_are_not() {
        assert!(TagSource::QuickTimeCreateDate.is_utc());
        assert!(TagSource::QuickTimeCreationDate.is_utc());
        assert!(TagSource::KeysCreationDate.is_utc());
        assert!(TagSource::QuickTimeModifyDate.is_utc());
        assert!(!TagSource::ExifDateTimeOriginal.is_utc());
        assert!(!TagSource::ExifCreateDate.is_utc());
        assert!(!TagSource::XmpCreateDate.is_utc());
    }

    #[test]
    fn orientation_covers_all_eight_values_and_knows_which_transpose() {
        assert_eq!(Orientation::from_exif(1), Orientation::Normal);
        assert_eq!(Orientation::from_exif(8), Orientation::Rotate270);
        assert_eq!(Orientation::from_exif(0), Orientation::Normal);
        assert_eq!(Orientation::from_exif(99), Orientation::Normal);

        for v in [5u16, 6, 7, 8] {
            assert!(Orientation::from_exif(v).swaps_axes(), "{v} swaps axes");
        }
        for v in [1u16, 2, 3, 4] {
            assert!(!Orientation::from_exif(v).swaps_axes(), "{v} does not");
        }
    }

    #[test]
    fn video_extensions_are_recognised_case_insensitively() {
        assert!(is_video(Path::new("clip.MOV")));
        assert!(is_video(Path::new("clip.mp4")));
        assert!(!is_video(Path::new("frame.jpg")));
        assert!(!is_video(Path::new("noext")));
    }
}

#[cfg(test)]
mod geo_tests {
    use super::*;

    #[test]
    fn an_offset_is_read_as_minutes_east_of_greenwich() {
        assert_eq!(parse_utc_offset("+02:00"), Some(120));
        assert_eq!(parse_utc_offset("-05:00"), Some(-300));
        assert_eq!(parse_utc_offset("+00:00"), Some(0));
    }

    #[test]
    fn a_zone_that_is_not_on_a_whole_hour_keeps_its_minutes() {
        // Kathmandu, Adelaide and the Chatham Islands are not rounding errors.
        assert_eq!(parse_utc_offset("+05:45"), Some(345));
        assert_eq!(parse_utc_offset("+09:30"), Some(570));
        assert_eq!(parse_utc_offset("-03:30"), Some(-210));
    }

    #[test]
    fn an_offset_arrives_quoted_from_exif_and_is_read_anyway() {
        assert_eq!(parse_utc_offset("\"+02:00\""), Some(120));
        assert_eq!(parse_utc_offset("  +02:00  "), Some(120));
    }

    #[test]
    fn something_that_is_not_an_offset_is_not_read_as_one() {
        // The tag exists and is blank on plenty of cameras. A blank read as
        // UTC would be a silent hour's error in every photograph.
        for raw in ["", " ", "+", "02:00:00 ", "+15:00", "+02:99", "Z", "+aa:bb"] {
            assert_eq!(parse_utc_offset(raw), None, "{raw:?} should not parse");
        }
    }

    #[test]
    fn a_file_with_no_gps_reports_no_gps_rather_than_a_position_at_null_island() {
        // 0°N 0°E is in the Gulf of Guinea, and it is where every "absent means
        // zero" bug puts a photograph.
        assert_eq!(MediaMeta::empty().gps, None);
    }
}
