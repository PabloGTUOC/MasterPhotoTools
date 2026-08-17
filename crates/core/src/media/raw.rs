//! RAW to JPEG (F14).
//!
//! An ordered ladder; **the first success wins**:
//!
//! 1. **Embedded preview.** Nearly every RAW carries a full-resolution JPEG
//!    rendered by the camera's own image engine — correct colour, correct tone
//!    curve, and effectively free to extract. The default and the preferred
//!    result.
//! 2. **macOS ImageIO**, Apple's own RAW pipeline, better than any pure-Rust
//!    decoder but per-camera-model and tied to the OS version.
//! 3. **`rawler`** — pure Rust, the fallback, and the only option on the Linux
//!    server.
//!
//! Rung 1 is where the quality is and rung 1 is portable, so it is the rung with
//! the tests. The ladder itself is built over a trait so a test can prove that a
//! file with a preview never reaches rungs 2 and 3 — an ordering claim needs
//! evidence that the later rungs were not *called*, not merely that an earlier
//! one produced the answer.

use crate::error::Error;
use std::path::Path;

/// Which rung produced the JPEG.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RawSource {
    EmbeddedPreview,
    MacOsImageIo,
    Rawler,
}

impl RawSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            RawSource::EmbeddedPreview => "embedded_preview",
            RawSource::MacOsImageIo => "macos_imageio",
            RawSource::Rawler => "rawler",
        }
    }
}

/// A JPEG produced from a RAW file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivedJpeg {
    pub bytes: Vec<u8>,
    pub source: RawSource,
    pub width: u32,
    pub height: u32,
}

/// The RAW containers F14 names.
///
/// **CR3 is deliberately absent.** It uses an ISO-BMFF container rather than a
/// TIFF-based one and needs separate handling; F14 puts it out of scope until
/// needed, and silently mis-handling it would be worse than declining it.
pub const RAW_EXTENSIONS: &[&str] = &["dng", "nef", "arw", "cr2", "raf"];

pub fn is_raw(path: &Path) -> bool {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .map(|e| RAW_EXTENSIONS.contains(&e.as_str()))
        .unwrap_or(false)
}

/// One rung of the ladder.
pub trait Rung: Send + Sync {
    fn name(&self) -> &'static str;

    /// `Ok(None)` means this rung cannot handle the file — try the next one.
    /// `Err` means it tried and failed, which also falls through but is worth
    /// reporting.
    fn attempt(&self, path: &Path) -> Result<Option<DerivedJpeg>, Error>;
}

/// F14's ladder, in order.
pub fn default_ladder() -> Vec<Box<dyn Rung>> {
    vec![
        Box::new(EmbeddedPreview),
        Box::new(MacOsImageIo),
        Box::new(RawlerDecode),
    ]
}

/// Try each rung in order and return the first success (F14).
///
/// A rung that errors is not fatal: the next rung still gets its turn, and the
/// reasons are collected so a failure at the end of the ladder can say what each
/// rung actually said rather than "could not convert".
pub fn run_ladder(path: &Path, rungs: &[Box<dyn Rung>]) -> Result<DerivedJpeg, Error> {
    let mut reasons = Vec::new();

    for rung in rungs {
        match rung.attempt(path) {
            Ok(Some(derived)) => return Ok(derived),
            Ok(None) => reasons.push(format!("{}: not applicable", rung.name())),
            Err(e) => reasons.push(format!("{}: {e}", rung.name())),
        }
    }

    Err(Error::Internal(format!(
        "could not derive a JPEG from {}: {}",
        path.display(),
        reasons.join("; ")
    )))
}

/// Convert one RAW file to a JPEG using F14's ladder.
pub fn raw_to_jpeg(path: &Path) -> Result<DerivedJpeg, Error> {
    run_ladder(path, &default_ladder())
}

// ---------------------------------------------------------------------------
// Rung 1 — the embedded preview
// ---------------------------------------------------------------------------

pub struct EmbeddedPreview;

impl Rung for EmbeddedPreview {
    fn name(&self) -> &'static str {
        "embedded preview"
    }

    fn attempt(&self, path: &Path) -> Result<Option<DerivedJpeg>, Error> {
        let bytes = std::fs::read(path)?;
        let Some(preview) = largest_embedded_jpeg(&bytes) else {
            return Ok(None);
        };

        // The camera wrote it, but a truncated card can still produce a JPEG
        // that will not decode. Measure it rather than trusting the header.
        let (width, height) = jpeg_dimensions(&preview).ok_or_else(|| {
            Error::Internal("the embedded preview is not a decodable JPEG".into())
        })?;

        Ok(Some(DerivedJpeg {
            bytes: preview,
            source: RawSource::EmbeddedPreview,
            width,
            height,
        }))
    }
}

/// The largest embedded JPEG in a TIFF-based RAW file.
///
/// "Largest" because a RAW typically carries several: a 160×120 thumbnail for
/// the camera's index view, sometimes a screen-sized preview, and the
/// full-resolution render. Only the last is worth publishing, and size is what
/// separates them.
pub fn largest_embedded_jpeg(bytes: &[u8]) -> Option<Vec<u8>> {
    let tiff = Tiff::parse(bytes)?;
    let mut best: Option<(usize, usize)> = None;

    for (offset, length) in tiff.jpeg_candidates() {
        let end = offset.checked_add(length)?;
        if end > bytes.len() || length < 4 {
            continue;
        }
        // A JPEG starts with SOI. Anything else is a pointer to something that
        // is not a picture.
        if bytes[offset] != 0xFF || bytes[offset + 1] != 0xD8 {
            continue;
        }
        if best.map(|(_, best_len)| length > best_len).unwrap_or(true) {
            best = Some((offset, length));
        }
    }

    let (offset, length) = best?;
    Some(bytes[offset..offset + length].to_vec())
}

/// Width and height of a JPEG, from its frame header.
///
/// Reads the SOF segment rather than decoding — the dimensions are in the
/// header, and decoding a 24 MP preview to learn its size would repeat the
/// mistake F11 exists to avoid.
fn jpeg_dimensions(jpeg: &[u8]) -> Option<(u32, u32)> {
    if jpeg.len() < 4 || jpeg[0] != 0xFF || jpeg[1] != 0xD8 {
        return None;
    }

    let mut i = 2;
    while i + 3 < jpeg.len() {
        if jpeg[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = jpeg[i + 1];
        // Padding and standalone markers carry no length field.
        if marker == 0xFF || matches!(marker, 0x01 | 0xD0..=0xD9) {
            i += 2;
            continue;
        }

        let length = u16::from_be_bytes([jpeg[i + 2], jpeg[i + 3]]) as usize;

        // Every SOFn except DHT (0xC4), JPG (0xC8) and DAC (0xCC) is a frame
        // header, and its payload begins with precision, height, width.
        if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            let p = i + 5;
            if p + 3 >= jpeg.len() {
                return None;
            }
            let height = u16::from_be_bytes([jpeg[p], jpeg[p + 1]]) as u32;
            let width = u16::from_be_bytes([jpeg[p + 2], jpeg[p + 3]]) as u32;
            return (width > 0 && height > 0).then_some((width, height));
        }

        i += 2 + length;
    }

    None
}

// ---------------------------------------------------------------------------
// A minimal TIFF/IFD reader
// ---------------------------------------------------------------------------

mod tag {
    pub const STRIP_OFFSETS: u16 = 0x0111;
    pub const STRIP_BYTE_COUNTS: u16 = 0x0117;
    pub const COMPRESSION: u16 = 0x0103;
    pub const SUB_IFDS: u16 = 0x014A;
    pub const JPEG_INTERCHANGE_FORMAT: u16 = 0x0201;
    pub const JPEG_INTERCHANGE_FORMAT_LENGTH: u16 = 0x0202;
    pub const EXIF_IFD: u16 = 0x8769;
}

/// JPEG compression values. 6 is the obsolete form, 7 the current one; both
/// appear in the wild and both mean the strip data is a JPEG.
const COMPRESSION_JPEG: &[u32] = &[6, 7];

/// How deep to follow SubIFD chains before concluding the file is malformed.
const MAX_DEPTH: usize = 6;

struct Tiff<'a> {
    bytes: &'a [u8],
    little_endian: bool,
    first_ifd: usize,
}

#[derive(Clone, Copy)]
struct Entry {
    tag: u16,
    kind: u16,
    count: u32,
    /// The raw four-byte value slot; either the value itself or an offset.
    slot: [u8; 4],
}

impl<'a> Tiff<'a> {
    fn parse(bytes: &'a [u8]) -> Option<Self> {
        if bytes.len() < 8 {
            return None;
        }
        let little_endian = match &bytes[0..2] {
            b"II" => true,
            b"MM" => false,
            _ => return None,
        };

        let magic = read_u16(bytes, 2, little_endian)?;
        if magic != 42 {
            return None;
        }

        let first_ifd = read_u32(bytes, 4, little_endian)? as usize;
        Some(Self {
            bytes,
            little_endian,
            first_ifd,
        })
    }

    /// Every `(offset, length)` pair in the file that might be a JPEG.
    fn jpeg_candidates(&self) -> Vec<(usize, usize)> {
        let mut found = Vec::new();
        let mut visited = Vec::new();
        self.walk(self.first_ifd, 0, &mut visited, &mut found);
        found
    }

    fn walk(
        &self,
        offset: usize,
        depth: usize,
        visited: &mut Vec<usize>,
        found: &mut Vec<(usize, usize)>,
    ) {
        // A malformed or hostile file can point an IFD at itself. Depth and a
        // visited set bound the walk either way.
        if depth > MAX_DEPTH || visited.contains(&offset) || offset == 0 {
            return;
        }
        visited.push(offset);

        let Some(entries) = self.read_ifd(offset) else {
            return;
        };

        let find = |t: u16| entries.iter().find(|e| e.tag == t).copied();

        // The classic preview pointer: JPEGInterchangeFormat plus its length.
        if let (Some(start), Some(length)) = (
            find(tag::JPEG_INTERCHANGE_FORMAT),
            find(tag::JPEG_INTERCHANGE_FORMAT_LENGTH),
        ) {
            if let (Some(o), Some(l)) = (self.scalar(&start), self.scalar(&length)) {
                found.push((o as usize, l as usize));
            }
        }

        // DNG and several makers store the full-size preview as JPEG strip data
        // in a SubIFD instead.
        let compression = find(tag::COMPRESSION).and_then(|e| self.scalar(&e));
        if compression
            .map(|c| COMPRESSION_JPEG.contains(&c))
            .unwrap_or(false)
        {
            if let (Some(offsets), Some(counts)) =
                (find(tag::STRIP_OFFSETS), find(tag::STRIP_BYTE_COUNTS))
            {
                // A single strip is a whole JPEG. Multiple strips are tiles of
                // one image and are not independently decodable, so they are
                // left to rung 3.
                if offsets.count == 1 && counts.count == 1 {
                    if let (Some(o), Some(l)) = (self.scalar(&offsets), self.scalar(&counts)) {
                        found.push((o as usize, l as usize));
                    }
                }
            }
        }

        // Recurse into SubIFDs, the Exif IFD, and the next IFD in the chain.
        if let Some(sub) = find(tag::SUB_IFDS) {
            for child in self.offsets(&sub) {
                self.walk(child as usize, depth + 1, visited, found);
            }
        }
        if let Some(exif) = find(tag::EXIF_IFD) {
            if let Some(child) = self.scalar(&exif) {
                self.walk(child as usize, depth + 1, visited, found);
            }
        }

        // IFD1 conventionally holds the thumbnail; it is a candidate like any
        // other, and being smallest it simply will not win.
        let count = self.entry_count(offset).unwrap_or(0) as usize;
        let next_at = offset + 2 + 12 * count;
        if let Some(next) = read_u32(self.bytes, next_at, self.little_endian) {
            self.walk(next as usize, depth + 1, visited, found);
        }
    }

    fn entry_count(&self, offset: usize) -> Option<u16> {
        read_u16(self.bytes, offset, self.little_endian)
    }

    fn read_ifd(&self, offset: usize) -> Option<Vec<Entry>> {
        let count = self.entry_count(offset)? as usize;
        // An entry count far larger than the file is a corrupt header, not an
        // IFD; reading it would be a very long walk through noise.
        if count == 0 || offset + 2 + 12 * count > self.bytes.len() {
            return None;
        }

        let mut entries = Vec::with_capacity(count);
        for i in 0..count {
            let at = offset + 2 + 12 * i;
            entries.push(Entry {
                tag: read_u16(self.bytes, at, self.little_endian)?,
                kind: read_u16(self.bytes, at + 2, self.little_endian)?,
                count: read_u32(self.bytes, at + 4, self.little_endian)?,
                slot: self.bytes.get(at + 8..at + 12)?.try_into().ok()?,
            });
        }
        Some(entries)
    }

    /// A one-element SHORT or LONG value.
    fn scalar(&self, entry: &Entry) -> Option<u32> {
        if entry.count != 1 {
            return None;
        }
        match entry.kind {
            3 => Some(read_u16_from(&entry.slot, 0, self.little_endian)? as u32),
            4 => read_u32_from(&entry.slot, 0, self.little_endian),
            _ => None,
        }
    }

    /// A LONG array, which is how SubIFD pointers are stored.
    fn offsets(&self, entry: &Entry) -> Vec<u32> {
        if entry.kind != 4 {
            return Vec::new();
        }
        if entry.count == 1 {
            return read_u32_from(&entry.slot, 0, self.little_endian)
                .into_iter()
                .collect();
        }

        let at = match read_u32_from(&entry.slot, 0, self.little_endian) {
            Some(a) => a as usize,
            None => return Vec::new(),
        };
        // Bounded by the file, so a corrupt count cannot ask for a huge vector.
        let count = (entry.count as usize).min(64);
        (0..count)
            .filter_map(|i| read_u32(self.bytes, at + i * 4, self.little_endian))
            .collect()
    }
}

fn read_u16(bytes: &[u8], at: usize, le: bool) -> Option<u16> {
    read_u16_from(bytes.get(at..at + 2)?, 0, le)
}

fn read_u32(bytes: &[u8], at: usize, le: bool) -> Option<u32> {
    read_u32_from(bytes.get(at..at + 4)?, 0, le)
}

fn read_u16_from(bytes: &[u8], at: usize, le: bool) -> Option<u16> {
    let b: [u8; 2] = bytes.get(at..at + 2)?.try_into().ok()?;
    Some(if le {
        u16::from_le_bytes(b)
    } else {
        u16::from_be_bytes(b)
    })
}

fn read_u32_from(bytes: &[u8], at: usize, le: bool) -> Option<u32> {
    let b: [u8; 4] = bytes.get(at..at + 4)?.try_into().ok()?;
    Some(if le {
        u32::from_le_bytes(b)
    } else {
        u32::from_be_bytes(b)
    })
}

// ---------------------------------------------------------------------------
// Rung 2 — macOS ImageIO
// ---------------------------------------------------------------------------

/// Apple's own RAW pipeline, reached through the system `sips` utility.
///
/// F14 names `objc2` bindings or `sips` as equally acceptable. `sips` is chosen
/// because the alternative is a substantial amount of unsafe FFI that **cannot
/// be exercised on this machine at all** — neither can `sips`, but a one-line
/// subprocess has far less that can be silently wrong.
///
/// Note that specification §2.6 calls `exiftool` "the one permitted external
/// binary" while F14 explicitly permits `sips`. The more specific and later
/// statement wins; this is recorded in the phase report.
pub struct MacOsImageIo;

impl Rung for MacOsImageIo {
    fn name(&self) -> &'static str {
        "macOS ImageIO"
    }

    #[cfg(not(target_os = "macos"))]
    fn attempt(&self, _path: &Path) -> Result<Option<DerivedJpeg>, Error> {
        // Not a failure. This rung does not exist away from macOS, which is
        // exactly why rung 3 does.
        Ok(None)
    }

    #[cfg(target_os = "macos")]
    fn attempt(&self, path: &Path) -> Result<Option<DerivedJpeg>, Error> {
        let temp =
            std::env::temp_dir().join(format!("phototools-imageio-{}.jpg", std::process::id()));

        let output = std::process::Command::new("sips")
            .arg("-s")
            .arg("format")
            .arg("jpeg")
            .arg(path)
            .arg("--out")
            .arg(&temp)
            .output();

        let output = match output {
            Ok(o) => o,
            // No `sips` at all is "not applicable", not a failure.
            Err(_) => return Ok(None),
        };

        if !output.status.success() {
            let _ = std::fs::remove_file(&temp);
            // Support is per-camera-model; a model ImageIO does not know is a
            // fallthrough to rung 3, not an error.
            return Ok(None);
        }

        let bytes = std::fs::read(&temp)?;
        let _ = std::fs::remove_file(&temp);

        let (width, height) = jpeg_dimensions(&bytes)
            .ok_or_else(|| Error::Internal("sips produced something that is not a JPEG".into()))?;

        Ok(Some(DerivedJpeg {
            bytes,
            source: RawSource::MacOsImageIo,
            width,
            height,
        }))
    }
}

// ---------------------------------------------------------------------------
// Rung 3 — rawler
// ---------------------------------------------------------------------------

/// The pure-Rust fallback, and the only rung available on the Linux server.
pub struct RawlerDecode;

impl Rung for RawlerDecode {
    fn name(&self) -> &'static str {
        "rawler"
    }

    fn attempt(&self, path: &Path) -> Result<Option<DerivedJpeg>, Error> {
        use rawler::imgop::develop::RawDevelop;

        // rawler panics on some malformed input rather than returning an error,
        // and a corrupt file on a card must not take the process down.
        let decoded = std::panic::catch_unwind(|| rawler::decode_file(path))
            .map_err(|_| Error::Internal("rawler panicked while decoding".into()))?;

        let raw = match decoded {
            Ok(raw) => raw,
            // A format rawler does not know is "not applicable".
            Err(_) => return Ok(None),
        };

        let developed = RawDevelop::default()
            .develop_intermediate(&raw)
            .map_err(|e| Error::Internal(format!("rawler could not develop the image: {e}")))?;

        let image = developed
            .to_dynamic_image()
            .ok_or_else(|| Error::Internal("rawler produced no image".into()))?;

        let width = image.width();
        let height = image.height();

        // The deliverable encoder: 4:4:4, maximum effort. This is a
        // photograph's only encode, so it is not the place to save milliseconds.
        let bytes =
            crate::media::jpeg::encode(&image, &crate::media::jpeg::JpegOptions::deliverable(92))?;

        Ok(Some(DerivedJpeg {
            bytes,
            source: RawSource::Rawler,
            width,
            height,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// A rung that records whether it was called, for asserting ladder order.
    struct Counting {
        name: &'static str,
        calls: Arc<AtomicUsize>,
        answer: Option<RawSource>,
    }

    impl Rung for Counting {
        fn name(&self) -> &'static str {
            self.name
        }

        fn attempt(&self, _path: &Path) -> Result<Option<DerivedJpeg>, Error> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.answer.map(|source| DerivedJpeg {
                bytes: vec![0xFF, 0xD8],
                source,
                width: 1,
                height: 1,
            }))
        }
    }

    fn counting(
        name: &'static str,
        answer: Option<RawSource>,
    ) -> (Box<dyn Rung>, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        (
            Box::new(Counting {
                name,
                calls: Arc::clone(&calls),
                answer,
            }),
            calls,
        )
    }

    #[test]
    fn the_first_rung_to_succeed_stops_the_ladder() {
        // F14's acceptance: with a preview present, steps 2 and 3 are not
        // reached. Asserted by counting calls, not by inspecting the winner.
        let (first, first_calls) = counting("one", Some(RawSource::EmbeddedPreview));
        let (second, second_calls) = counting("two", Some(RawSource::MacOsImageIo));
        let (third, third_calls) = counting("three", Some(RawSource::Rawler));

        let result = run_ladder(&PathBuf::from("unused"), &[first, second, third]).unwrap();

        assert_eq!(result.source, RawSource::EmbeddedPreview);
        assert_eq!(first_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            second_calls.load(Ordering::SeqCst),
            0,
            "rung 2 must not run"
        );
        assert_eq!(third_calls.load(Ordering::SeqCst), 0, "rung 3 must not run");
    }

    #[test]
    fn a_rung_that_cannot_help_falls_through_to_the_next() {
        let (first, first_calls) = counting("one", None);
        let (second, second_calls) = counting("two", Some(RawSource::Rawler));

        let result = run_ladder(&PathBuf::from("unused"), &[first, second]).unwrap();

        assert_eq!(result.source, RawSource::Rawler);
        assert_eq!(first_calls.load(Ordering::SeqCst), 1);
        assert_eq!(second_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_rung_that_errors_does_not_stop_the_ladder() {
        struct Broken;
        impl Rung for Broken {
            fn name(&self) -> &'static str {
                "broken"
            }
            fn attempt(&self, _path: &Path) -> Result<Option<DerivedJpeg>, Error> {
                Err(Error::Internal("the decoder fell over".into()))
            }
        }

        let (last, last_calls) = counting("last", Some(RawSource::Rawler));
        let result = run_ladder(&PathBuf::from("unused"), &[Box::new(Broken), last]).unwrap();

        assert_eq!(result.source, RawSource::Rawler);
        assert_eq!(last_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_ladder_that_runs_out_says_what_each_rung_said() {
        struct Broken;
        impl Rung for Broken {
            fn name(&self) -> &'static str {
                "broken"
            }
            fn attempt(&self, _path: &Path) -> Result<Option<DerivedJpeg>, Error> {
                Err(Error::Internal("the decoder fell over".into()))
            }
        }
        let (skipped, _) = counting("skipped", None);

        let err = run_ladder(&PathBuf::from("some.nef"), &[Box::new(Broken), skipped]).unwrap_err();
        let message = err.to_string();

        assert!(message.contains("the decoder fell over"), "{message}");
        assert!(message.contains("not applicable"), "{message}");
        assert!(message.contains("some.nef"), "{message}");
    }

    #[test]
    fn the_default_ladder_is_in_f14s_order() {
        let ladder = default_ladder();
        let names: Vec<&str> = ladder.iter().map(|r| r.name()).collect();
        assert_eq!(names, vec!["embedded preview", "macOS ImageIO", "rawler"]);
    }

    #[test]
    fn f14s_formats_are_recognised_and_cr3_is_not() {
        for name in ["a.dng", "a.NEF", "a.arw", "a.CR2", "a.raf"] {
            assert!(is_raw(Path::new(name)), "{name} is one of F14's formats");
        }
        // F14 puts CR3 out of scope: it is ISO-BMFF, not TIFF-based.
        assert!(!is_raw(Path::new("a.cr3")));
        assert!(!is_raw(Path::new("a.jpg")));
        assert!(!is_raw(Path::new("noextension")));
    }

    // ------------------------------------------------------- jpeg dimensions

    #[test]
    fn jpeg_dimensions_come_from_the_frame_header() {
        let img = image::DynamicImage::new_rgb8(640, 480);
        let bytes =
            crate::media::jpeg::encode(&img, &crate::media::jpeg::JpegOptions::fast(80)).unwrap();

        assert_eq!(jpeg_dimensions(&bytes), Some((640, 480)));
    }

    #[test]
    fn something_that_is_not_a_jpeg_has_no_dimensions() {
        assert_eq!(jpeg_dimensions(b"not a jpeg at all"), None);
        assert_eq!(jpeg_dimensions(&[0xFF, 0xD8]), None);
        assert_eq!(jpeg_dimensions(&[]), None);
    }

    // -------------------------------------------------------- the TIFF walker

    #[test]
    fn a_file_that_is_not_a_tiff_yields_no_candidates() {
        assert!(largest_embedded_jpeg(b"not a tiff").is_none());
        assert!(largest_embedded_jpeg(&[]).is_none());
        // A JPEG is not a TIFF, and asking for its embedded preview is not an
        // error — it simply has none.
        assert!(largest_embedded_jpeg(&[0xFF, 0xD8, 0xFF, 0xE0]).is_none());
    }

    #[test]
    fn a_truncated_tiff_header_is_survived() {
        assert!(largest_embedded_jpeg(b"II").is_none());
        assert!(largest_embedded_jpeg(b"II\x2a\x00").is_none());
        // A first-IFD offset past the end of the file.
        assert!(largest_embedded_jpeg(b"II\x2a\x00\xff\xff\xff\xff").is_none());
    }

    #[test]
    fn an_ifd_pointing_at_itself_terminates() {
        // Offset 8 is the IFD; its next-IFD pointer points back at 8.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"II");
        bytes.extend_from_slice(&42u16.to_le_bytes());
        bytes.extend_from_slice(&8u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes()); // one entry
        bytes.extend_from_slice(&0x0100u16.to_le_bytes());
        bytes.extend_from_slice(&4u16.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&64u32.to_le_bytes());
        bytes.extend_from_slice(&8u32.to_le_bytes()); // next IFD = itself

        // The visited set bounds it; without one this would not return.
        assert!(largest_embedded_jpeg(&bytes).is_none());
    }
}
