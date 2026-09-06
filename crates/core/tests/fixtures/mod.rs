//! Test fixture generator (build plan §5).
//!
//! The agent has no photographs, so every fixture is synthesised at test time
//! into a temp directory and never committed. Fixtures assert *measurable*
//! properties — known dimensions, a known capture date, a divider planted at a
//! known column — so tests can check exact values rather than eyeballing output.
//!
//! EXIF is written here by hand rather than by shelling out to `exiftool`, for
//! two reasons: the metadata tests must not depend on the tool whose output they
//! are validating, and spawning a process per fixture makes the suite crawl.

#![allow(dead_code)]

use image::{ImageBuffer, Rgb, RgbImage, Rgba, RgbaImage};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// A minimal TIFF/EXIF writer
// ---------------------------------------------------------------------------

/// The three TIFF value types these fixtures need.
#[derive(Clone, Debug)]
pub enum TiffValue {
    Ascii(String),
    Short(u16),
    Long(u32),
}

impl TiffValue {
    fn type_code(&self) -> u16 {
        match self {
            TiffValue::Ascii(_) => 2,
            TiffValue::Short(_) => 3,
            TiffValue::Long(_) => 4,
        }
    }

    fn count(&self) -> u32 {
        match self {
            // ASCII counts include the trailing NUL.
            TiffValue::Ascii(s) => s.len() as u32 + 1,
            _ => 1,
        }
    }

    /// True when an ASCII value, including its NUL, exceeds the inline slot.
    fn ascii_needs_overflow(s: &str) -> bool {
        let with_nul = s.len() + 1;
        with_nul > 4
    }

    /// Bytes for the value area, when it does not fit in the 4-byte inline slot.
    fn overflow_bytes(&self) -> Option<Vec<u8>> {
        match self {
            TiffValue::Ascii(s) if Self::ascii_needs_overflow(s) => {
                let mut b = s.as_bytes().to_vec();
                b.push(0);
                Some(b)
            }
            _ => None,
        }
    }

    /// The 4-byte inline slot, when the value fits.
    fn inline_bytes(&self) -> Option<[u8; 4]> {
        let mut slot = [0u8; 4];
        match self {
            TiffValue::Ascii(s) if !Self::ascii_needs_overflow(s) => {
                let b = s.as_bytes();
                slot[..b.len()].copy_from_slice(b);
                Some(slot)
            }
            TiffValue::Ascii(_) => None,
            TiffValue::Short(v) => {
                slot[..2].copy_from_slice(&v.to_le_bytes());
                Some(slot)
            }
            TiffValue::Long(v) => {
                slot.copy_from_slice(&v.to_le_bytes());
                Some(slot)
            }
        }
    }
}

/// Tag numbers used by the fixtures.
pub mod tag {
    pub const IMAGE_WIDTH: u16 = 0x0100;
    pub const IMAGE_HEIGHT: u16 = 0x0101;
    pub const MAKE: u16 = 0x010F;
    pub const MODEL: u16 = 0x0110;
    pub const ORIENTATION: u16 = 0x0112;
    pub const JPEG_INTERCHANGE_FORMAT: u16 = 0x0201;
    pub const JPEG_INTERCHANGE_FORMAT_LENGTH: u16 = 0x0202;
    pub const MODIFY_DATE: u16 = 0x0132;
    pub const EXIF_OFFSET: u16 = 0x8769;
    pub const DATE_TIME_ORIGINAL: u16 = 0x9003;
    pub const CREATE_DATE: u16 = 0x9004;
    pub const OFFSET_TIME: u16 = 0x9010;
    pub const OFFSET_TIME_ORIGINAL: u16 = 0x9011;
    pub const LENS_MODEL: u16 = 0xA434;
    pub const PIXEL_X_DIMENSION: u16 = 0xA002;
    pub const PIXEL_Y_DIMENSION: u16 = 0xA003;
}

/// Build a little-endian TIFF structure with an IFD0 and an optional Exif SubIFD.
///
/// Returns bytes starting at the TIFF header (`II*\0`), which is what both a
/// JPEG APP1 payload and a standalone TIFF file need. `extra_payload` is
/// appended after the value area and its start offset is handed to the caller so
/// an embedded JPEG preview can be pointed at.
fn build_tiff(
    ifd0: &[(u16, TiffValue)],
    exif_ifd: &[(u16, TiffValue)],
    extra_payload: Option<&[u8]>,
    payload_offset_tag: Option<u16>,
) -> Vec<u8> {
    let has_exif = !exif_ifd.is_empty();
    let mut ifd0 = ifd0.to_vec();

    // Reserve the ExifOffset entry so the entry count is right before we know
    // where the Exif IFD lands.
    if has_exif {
        ifd0.push((tag::EXIF_OFFSET, TiffValue::Long(0)));
    }
    // Reserve the payload pointer entry likewise.
    if extra_payload.is_some() {
        if let Some(t) = payload_offset_tag {
            ifd0.push((t, TiffValue::Long(0)));
        }
    }
    ifd0.sort_by_key(|(t, _)| *t);

    let header_len = 8usize;
    let ifd0_len = 2 + 12 * ifd0.len() + 4;
    let exif_len = if has_exif {
        2 + 12 * exif_ifd.len() + 4
    } else {
        0
    };

    let ifd0_offset = header_len;
    let exif_offset = ifd0_offset + ifd0_len;
    let value_area_offset = exif_offset + exif_len;

    // Lay out the overflow value area for both IFDs.
    let mut value_area = Vec::new();
    let mut ifd0_slots: Vec<[u8; 4]> = Vec::with_capacity(ifd0.len());
    let mut exif_slots: Vec<[u8; 4]> = Vec::with_capacity(exif_ifd.len());

    let place = |v: &TiffValue, value_area: &mut Vec<u8>| -> [u8; 4] {
        if let Some(inline) = v.inline_bytes() {
            inline
        } else {
            let at = (value_area_offset + value_area.len()) as u32;
            value_area.extend_from_slice(&v.overflow_bytes().unwrap());
            if value_area.len() % 2 == 1 {
                value_area.push(0); // keep word alignment
            }
            at.to_le_bytes()
        }
    };

    for (_, v) in &ifd0 {
        ifd0_slots.push(place(v, &mut value_area));
    }
    for (_, v) in exif_ifd {
        exif_slots.push(place(v, &mut value_area));
    }

    let payload_offset = value_area_offset + value_area.len();

    // Patch the reserved pointer entries now that offsets are known.
    for (i, (t, _)) in ifd0.iter().enumerate() {
        if has_exif && *t == tag::EXIF_OFFSET {
            ifd0_slots[i] = (exif_offset as u32).to_le_bytes();
        }
        if let (Some(pt), Some(_)) = (payload_offset_tag, extra_payload) {
            if *t == pt {
                ifd0_slots[i] = (payload_offset as u32).to_le_bytes();
            }
        }
    }

    let mut out = Vec::new();
    out.extend_from_slice(b"II");
    out.extend_from_slice(&42u16.to_le_bytes());
    out.extend_from_slice(&(ifd0_offset as u32).to_le_bytes());

    let write_ifd = |out: &mut Vec<u8>, entries: &[(u16, TiffValue)], slots: &[[u8; 4]]| {
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        for (i, (t, v)) in entries.iter().enumerate() {
            out.extend_from_slice(&t.to_le_bytes());
            out.extend_from_slice(&v.type_code().to_le_bytes());
            out.extend_from_slice(&v.count().to_le_bytes());
            out.extend_from_slice(&slots[i]);
        }
        out.extend_from_slice(&0u32.to_le_bytes()); // no next IFD
    };

    write_ifd(&mut out, &ifd0, &ifd0_slots);
    if has_exif {
        write_ifd(&mut out, exif_ifd, &exif_slots);
    }
    out.extend_from_slice(&value_area);
    if let Some(p) = extra_payload {
        out.extend_from_slice(p);
    }
    out
}

/// Wrap a TIFF structure as a JPEG APP1 Exif segment.
fn app1_segment(tiff: &[u8]) -> Vec<u8> {
    let mut payload = b"Exif\x00\x00".to_vec();
    payload.extend_from_slice(tiff);

    let mut seg = vec![0xFF, 0xE1];
    seg.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
    seg.extend_from_slice(&payload);
    seg
}

/// Splice an APP1 segment in immediately after the JPEG SOI marker.
fn insert_app1(jpeg: &[u8], app1: &[u8]) -> Vec<u8> {
    assert_eq!(&jpeg[..2], &[0xFF, 0xD8], "not a JPEG");
    let mut out = Vec::with_capacity(jpeg.len() + app1.len());
    out.extend_from_slice(&jpeg[..2]);
    out.extend_from_slice(app1);
    out.extend_from_slice(&jpeg[2..]);
    out
}

fn encode_jpeg(img: &RgbImage, quality: u8) -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let mut cursor = Cursor::new(&mut buf);
        let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut cursor, quality);
        enc.encode_image(&image::DynamicImage::ImageRgb8(img.clone()))
            .unwrap();
    }
    buf
}

/// A recognisable image: a coloured field with a brighter block, so a resize or
/// a rotation is visible in pixel statistics rather than only in dimensions.
fn patterned(w: u32, h: u32, base: Rgb<u8>) -> RgbImage {
    let mut img: RgbImage = ImageBuffer::from_pixel(w, h, base);
    for y in 0..h / 2 {
        for x in 0..w / 2 {
            img.put_pixel(x, y, Rgb([255, 255, 255]));
        }
    }
    img
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

pub struct Fixtures {
    pub temp: TempDir,
}

/// What `card_tree` should produce for one shot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShotKind {
    /// A JPEG and a RAW sharing one stem — one shot, two assets.
    RawPlusJpeg,
    JpegOnly,
    RawOnly,
}

impl Default for Fixtures {
    fn default() -> Self {
        Self::new()
    }
}

impl Fixtures {
    pub fn new() -> Self {
        Self {
            temp: tempfile::tempdir().unwrap(),
        }
    }

    pub fn path(&self) -> &Path {
        self.temp.path()
    }

    /// A JPEG of known size carrying a known capture date and camera.
    ///
    /// `capture` is `YYYY:MM:DD HH:MM:SS`. Writes both `DateTimeOriginal` and
    /// `CreateDate`, plus `PixelXDimension`/`PixelYDimension` matching the real
    /// pixel size, so the EXIF-preservation round trip has something to check.
    pub fn jpeg_with_exif(
        &self,
        name: &str,
        w: u32,
        h: u32,
        capture: &str,
        camera: &str,
    ) -> PathBuf {
        self.jpeg_with_tags(
            name,
            w,
            h,
            &[
                (tag::MODEL, TiffValue::Ascii(camera.to_string())),
                (tag::ORIENTATION, TiffValue::Short(1)),
            ],
            &[
                (
                    tag::DATE_TIME_ORIGINAL,
                    TiffValue::Ascii(capture.to_string()),
                ),
                (tag::CREATE_DATE, TiffValue::Ascii(capture.to_string())),
                (tag::PIXEL_X_DIMENSION, TiffValue::Long(w)),
                (tag::PIXEL_Y_DIMENSION, TiffValue::Long(h)),
            ],
        )
    }

    /// A JPEG with an arbitrary tag set — for tag-priority and normalisation tests.
    pub fn jpeg_with_tags(
        &self,
        name: &str,
        w: u32,
        h: u32,
        ifd0: &[(u16, TiffValue)],
        exif_ifd: &[(u16, TiffValue)],
    ) -> PathBuf {
        let path = self.temp.path().join(name);
        let img = patterned(w, h, Rgb([40, 90, 140]));
        let jpeg = encode_jpeg(&img, 92);
        let tiff = build_tiff(ifd0, exif_ifd, None, None);
        let with_exif = insert_app1(&jpeg, &app1_segment(&tiff));
        std::fs::write(&path, with_exif).unwrap();
        path
    }

    /// A JPEG with a given EXIF orientation value (1–8) and no other metadata.
    pub fn jpeg_with_orientation(&self, name: &str, w: u32, h: u32, orientation: u16) -> PathBuf {
        self.jpeg_with_tags(
            name,
            w,
            h,
            &[(tag::ORIENTATION, TiffValue::Short(orientation))],
            &[
                (tag::PIXEL_X_DIMENSION, TiffValue::Long(w)),
                (tag::PIXEL_Y_DIMENSION, TiffValue::Long(h)),
            ],
        )
    }

    /// A JPEG whose metadata is intact but whose pixel data cannot be decoded.
    ///
    /// This exists to make F11's "dimensions come from metadata, never by
    /// decoding" assertion mean something. Anything that decodes this file
    /// fails; anything that reads its metadata gets `w`×`h`. A scan that passes
    /// on this fixture demonstrably did not decode.
    ///
    /// The APP1 segment carrying EXIF is left exactly as the encoder wrote it,
    /// and everything from the start-of-frame marker onwards is dropped. The
    /// result is a well-formed EXIF container with no frame header, so no
    /// decoder can determine even the image's size, while any metadata reader
    /// finds the full tag set.
    ///
    /// Corrupting the entropy-coded scan data instead does *not* work: a run of
    /// `0xFF` reads as JPEG fill bytes, and decoders tolerate a truncated scan
    /// by returning whatever they managed to reconstruct.
    pub fn jpeg_with_unreadable_pixels(&self, name: &str, w: u32, h: u32) -> PathBuf {
        let path = self.temp.path().join(name);
        let intact = std::fs::read(self.jpeg_with_exif(
            &format!("__tmp_broken_{name}"),
            w,
            h,
            "2024:05:01 09:00:00",
            "CANON EOS R6",
        ))
        .unwrap();

        // Every SOFn marker — baseline, progressive and the arithmetic-coded
        // variants — excluding the markers in that range that are not frame
        // headers (DHT 0xC4, JPG 0xC8, DAC 0xCC).
        let sof = intact
            .windows(2)
            .position(|w| {
                w[0] == 0xFF && (0xC0..=0xCF).contains(&w[1]) && !matches!(w[1], 0xC4 | 0xC8 | 0xCC)
            })
            .expect("a JPEG the encoder produced has a frame header");

        let mut broken = intact[..sof].to_vec();
        broken.extend_from_slice(&[0xFF, 0xD9]); // EOI
        std::fs::write(&path, broken).unwrap();
        path
    }

    /// A JPEG with no metadata at all.
    pub fn jpeg_without_exif(&self, name: &str, w: u32, h: u32) -> PathBuf {
        let path = self.temp.path().join(name);
        let img = patterned(w, h, Rgb([120, 60, 30]));
        std::fs::write(&path, encode_jpeg(&img, 92)).unwrap();
        path
    }

    pub fn png(&self, name: &str, w: u32, h: u32) -> PathBuf {
        let path = self.temp.path().join(name);
        patterned(w, h, Rgb([20, 140, 90])).save(&path).unwrap();
        path
    }

    /// A synthetic two-up half-frame scan (F4).
    ///
    /// Layout, left to right: `border` px of white lab surround, a coloured
    /// panel, a `divider_w` px dark divider, a second differently-coloured
    /// panel, then white surround again. The same white border runs top and
    /// bottom. The returned value is the exact x of the divider's centre so the
    /// detector can be checked against ground truth.
    pub fn half_frame_scan(
        &self,
        name: &str,
        panel_w: u32,
        panel_h: u32,
        border: u32,
        divider_w: u32,
    ) -> (PathBuf, u32) {
        let w = border * 2 + panel_w * 2 + divider_w;
        let h = border * 2 + panel_h;
        let mut img: RgbImage = ImageBuffer::from_pixel(w, h, Rgb([250, 250, 250]));

        let left_x = border;
        let divider_x = border + panel_w;
        let right_x = divider_x + divider_w;

        for y in border..border + panel_h {
            for x in left_x..left_x + panel_w {
                img.put_pixel(x, y, Rgb([190, 70, 60]));
            }
            for x in divider_x..divider_x + divider_w {
                img.put_pixel(x, y, Rgb([6, 6, 6]));
            }
            for x in right_x..right_x + panel_w {
                img.put_pixel(x, y, Rgb([60, 90, 190]));
            }
        }

        let path = self.temp.path().join(name);
        std::fs::write(&path, encode_jpeg(&img, 95)).unwrap();
        (path, divider_x + divider_w / 2)
    }

    /// A multi-page TIFF (F8).
    pub fn multipage_tiff(&self, name: &str, pages: u32, w: u32, h: u32) -> PathBuf {
        let path = self.temp.path().join(name);
        let file = std::fs::File::create(&path).unwrap();
        let mut encoder = tiff::encoder::TiffEncoder::new(std::io::BufWriter::new(file)).unwrap();

        for page in 0..pages {
            // Each page a distinguishable flat colour.
            let shade = (40 + page * 40).min(255) as u8;
            let data: Vec<u8> = (0..w * h).flat_map(|_| [shade, shade, shade]).collect();
            let img = encoder
                .new_image::<tiff::encoder::colortype::RGB8>(w, h)
                .unwrap();
            img.write_data(&data).unwrap();
        }
        path
    }

    /// A single-page TIFF with an alpha channel, for the flatten-onto-white path.
    pub fn tiff_with_alpha(&self, name: &str, w: u32, h: u32) -> PathBuf {
        let path = self.temp.path().join(name);
        // Fully transparent black: if alpha is ignored the result is black, if
        // it is flattened onto white the result is white. Unmissable either way.
        let img: RgbaImage = ImageBuffer::from_pixel(w, h, Rgba([0, 0, 0, 0]));
        img.save(&path).unwrap();
        path
    }

    /// A minimal TIFF/IFD carrying an embedded full-resolution JPEG preview,
    /// the shape F14's preferred path walks.
    ///
    /// Returns the path and the exact preview bytes, so a test can assert the
    /// extracted preview is byte-identical.
    /// A TIFF-based RAW stub carrying an embedded preview and full metadata.
    ///
    /// For F14: the ladder extracts the preview, and the capture date, camera
    /// and lens must survive from the RAW into the derived JPEG. The preview
    /// itself deliberately carries **no** metadata, so a test that finds a date
    /// in the output can only have got it from the copy step.
    pub fn raw_with_metadata(
        &self,
        name: &str,
        w: u32,
        h: u32,
        capture: &str,
        camera: &str,
        lens: &str,
    ) -> PathBuf {
        let preview = encode_jpeg(&patterned(w, h, Rgb([200, 160, 40])), 90);
        let tiff = build_tiff(
            &[
                (tag::IMAGE_WIDTH, TiffValue::Long(w)),
                (tag::IMAGE_HEIGHT, TiffValue::Long(h)),
                (tag::MAKE, TiffValue::Ascii("STUBMAKER".into())),
                (tag::MODEL, TiffValue::Ascii(camera.to_string())),
                (
                    tag::JPEG_INTERCHANGE_FORMAT_LENGTH,
                    TiffValue::Long(preview.len() as u32),
                ),
            ],
            &[
                (
                    tag::DATE_TIME_ORIGINAL,
                    TiffValue::Ascii(capture.to_string()),
                ),
                (tag::CREATE_DATE, TiffValue::Ascii(capture.to_string())),
                (tag::LENS_MODEL, TiffValue::Ascii(lens.to_string())),
            ],
            Some(&preview),
            Some(tag::JPEG_INTERCHANGE_FORMAT),
        );

        let path = self.temp.path().join(name);
        std::fs::write(&path, &tiff).unwrap();
        path
    }

    /// A RAW stub carrying **two** embedded JPEGs: a small thumbnail in IFD0 and
    /// a larger preview in IFD1.
    ///
    /// F14 prefers the full-resolution render, so the extractor must pick the
    /// larger one. A file with only one preview cannot demonstrate that.
    ///
    /// Returns the path and the bytes of the preview that ought to win.
    pub fn raw_with_thumbnail_and_preview(&self, name: &str) -> (PathBuf, Vec<u8>) {
        let thumbnail = encode_jpeg(&patterned(160, 120, Rgb([30, 30, 30])), 70);
        let preview = encode_jpeg(&patterned(1600, 1200, Rgb([200, 160, 40])), 90);

        // IFD0 points at the thumbnail; IFD1 — the next IFD in the chain —
        // points at the larger preview, which is how several makers lay it out.
        let mut out = Vec::new();
        out.extend_from_slice(b"II");
        out.extend_from_slice(&42u16.to_le_bytes());
        out.extend_from_slice(&8u32.to_le_bytes());

        let ifd_len = 2 + 12 * 3 + 4;
        let ifd0_at = 8usize;
        let ifd1_at = ifd0_at + ifd_len;
        let thumbnail_at = ifd1_at + ifd_len;
        let preview_at = thumbnail_at + thumbnail.len();

        let write_ifd = |out: &mut Vec<u8>, jpeg_at: usize, jpeg_len: usize, next: u32| {
            out.extend_from_slice(&3u16.to_le_bytes());
            for (tag, kind, count, value) in [
                (tag::MODEL, 3u16, 1u32, 1u32),
                (tag::JPEG_INTERCHANGE_FORMAT, 4, 1, jpeg_at as u32),
                (tag::JPEG_INTERCHANGE_FORMAT_LENGTH, 4, 1, jpeg_len as u32),
            ] {
                out.extend_from_slice(&tag.to_le_bytes());
                out.extend_from_slice(&kind.to_le_bytes());
                out.extend_from_slice(&count.to_le_bytes());
                out.extend_from_slice(&value.to_le_bytes());
            }
            out.extend_from_slice(&next.to_le_bytes());
        };

        write_ifd(&mut out, thumbnail_at, thumbnail.len(), ifd1_at as u32);
        write_ifd(&mut out, preview_at, preview.len(), 0);
        out.extend_from_slice(&thumbnail);
        out.extend_from_slice(&preview);

        let path = self.temp.path().join(name);
        std::fs::write(&path, &out).unwrap();
        (path, preview)
    }

    /// A RAW stub with no embedded preview at all — the case that must fall
    /// through F14's ladder.
    pub fn raw_without_preview(&self, name: &str, w: u32, h: u32) -> PathBuf {
        let tiff = build_tiff(
            &[
                (tag::IMAGE_WIDTH, TiffValue::Long(w)),
                (tag::IMAGE_HEIGHT, TiffValue::Long(h)),
                (tag::MODEL, TiffValue::Ascii("STUBCAM".into())),
            ],
            &[],
            None,
            None,
        );
        let path = self.temp.path().join(name);
        std::fs::write(&path, &tiff).unwrap();
        path
    }

    pub fn raw_stub_with_preview(&self, name: &str, w: u32, h: u32) -> (PathBuf, Vec<u8>) {
        let preview = encode_jpeg(&patterned(w, h, Rgb([200, 160, 40])), 90);
        let tiff = build_tiff(
            &[
                (tag::IMAGE_WIDTH, TiffValue::Long(w)),
                (tag::IMAGE_HEIGHT, TiffValue::Long(h)),
                (tag::MODEL, TiffValue::Ascii("STUBCAM".into())),
                (
                    tag::JPEG_INTERCHANGE_FORMAT_LENGTH,
                    TiffValue::Long(preview.len() as u32),
                ),
            ],
            &[],
            Some(&preview),
            Some(tag::JPEG_INTERCHANGE_FORMAT),
        );

        let path = self.temp.path().join(name);
        std::fs::write(&path, &tiff).unwrap();
        (path, preview)
    }

    /// A minimal QuickTime file whose `mvhd` carries a creation time.
    ///
    /// `epoch_secs` is a normal Unix timestamp; QuickTime's 1904 epoch offset is
    /// applied here.
    pub fn quicktime(&self, name: &str, epoch_secs: i64) -> PathBuf {
        const EPOCH_1904_TO_1970: i64 = 2_082_844_800;
        let qt_time = (epoch_secs + EPOCH_1904_TO_1970) as u32;

        let mut mvhd = Vec::new();
        mvhd.extend_from_slice(&[0, 0, 0, 0]); // version 0 + flags
        mvhd.extend_from_slice(&qt_time.to_be_bytes()); // creation_time
        mvhd.extend_from_slice(&qt_time.to_be_bytes()); // modification_time
        mvhd.extend_from_slice(&1000u32.to_be_bytes()); // timescale
        mvhd.extend_from_slice(&5000u32.to_be_bytes()); // duration
        mvhd.extend_from_slice(&0x0001_0000u32.to_be_bytes()); // rate 1.0
        mvhd.extend_from_slice(&0x0100u16.to_be_bytes()); // volume 1.0
        mvhd.extend_from_slice(&[0u8; 10]); // reserved
        let matrix: [u32; 9] = [0x0001_0000, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000];
        for m in matrix {
            mvhd.extend_from_slice(&m.to_be_bytes());
        }
        mvhd.extend_from_slice(&[0u8; 24]); // predefined
        mvhd.extend_from_slice(&2u32.to_be_bytes()); // next_track_id

        let mvhd_box = box_of(b"mvhd", &mvhd);
        let moov_box = box_of(b"moov", &mvhd_box);

        let mut ftyp = Vec::new();
        ftyp.extend_from_slice(b"qt  ");
        ftyp.extend_from_slice(&0u32.to_be_bytes());
        ftyp.extend_from_slice(b"qt  ");
        let ftyp_box = box_of(b"ftyp", &ftyp);

        let path = self.temp.path().join(name);
        let mut out = ftyp_box;
        out.extend_from_slice(&moov_box);
        std::fs::write(&path, out).unwrap();
        path
    }

    /// A media file plus its Google Takeout JSON sidecar (F2).
    ///
    /// `variant` selects which of Takeout's naming quirks to reproduce.
    pub fn takeout_pair(
        &self,
        media_name: &str,
        timestamp: i64,
        variant: TakeoutVariant,
    ) -> PathBuf {
        let media = self.jpeg_without_exif(media_name, 32, 32);

        let sidecar_name = match variant {
            TakeoutVariant::Exact => format!("{media_name}.json"),
            TakeoutVariant::Truncated { to } => {
                let mut n = media_name.to_string();
                n.truncate(to);
                format!("{n}.json")
            }
            TakeoutVariant::SuffixOnSidecar => {
                // photo(1).jpg -> photo.jpg(1).json
                let stripped = media_name.replace("(1)", "");
                let (stem, ext) = stripped.rsplit_once('.').unwrap();
                format!("{stem}.{ext}(1).json")
            }
            TakeoutVariant::SuffixOnMediaOnly => {
                let stripped = media_name.replace("(1)", "");
                format!("{stripped}.json")
            }
        };

        let json = format!(
            r#"{{"title":"{media_name}","photoTakenTime":{{"timestamp":"{timestamp}","formatted":"x"}}}}"#
        );
        std::fs::write(self.temp.path().join(sidecar_name), json).unwrap();
        media
    }

    /// A `DCIM` tree with configurable RAW+JPEG pairing (F11).
    ///
    /// Every JPEG is a real decodable image carrying EXIF, so a scan can read
    /// dimensions and capture dates from metadata as F11 requires.
    pub fn card_tree(&self, shots: &[ShotKind]) -> PathBuf {
        self.card_tree_named("card", shots)
    }

    pub fn card_tree_named(&self, dir_name: &str, shots: &[ShotKind]) -> PathBuf {
        let root = self.temp.path().join(dir_name);
        let dcim = root.join("DCIM").join("100CANON");
        std::fs::create_dir_all(&dcim).unwrap();

        for (i, kind) in shots.iter().enumerate() {
            let stem = format!("IMG_{:04}", i);
            let capture = format!("2024:05:01 {:02}:{:02}:00", 8 + i / 60, i % 60);

            if matches!(kind, ShotKind::RawPlusJpeg | ShotKind::JpegOnly) {
                let jpeg = self.jpeg_with_exif(
                    &format!("__tmp_{stem}.jpg"),
                    64,
                    48,
                    &capture,
                    "CANON EOS R6",
                );
                std::fs::rename(&jpeg, dcim.join(format!("{stem}.JPG"))).unwrap();
            }
            if matches!(kind, ShotKind::RawPlusJpeg | ShotKind::RawOnly) {
                let (raw, _) = self.raw_stub_with_preview(&format!("__tmp_{stem}.raw"), 64, 48);
                std::fs::rename(&raw, dcim.join(format!("{stem}.CR2"))).unwrap();
            }
        }

        // Junk a real card carries, which a scan must ignore.
        std::fs::write(root.join(".DS_Store"), "junk").unwrap();
        std::fs::create_dir_all(root.join("MISC")).unwrap();
        std::fs::write(root.join("MISC").join("AUTPRINT.MRK"), "junk").unwrap();

        root
    }
}

#[derive(Clone, Copy, Debug)]
pub enum TakeoutVariant {
    /// `photo.jpg` → `photo.jpg.json`
    Exact,
    /// Takeout truncates long names.
    Truncated { to: usize },
    /// `photo(1).jpg` → `photo.jpg(1).json`
    SuffixOnSidecar,
    /// `photo(1).jpg` → `photo.jpg.json`
    SuffixOnMediaOnly,
}

fn box_of(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 8);
    out.extend_from_slice(&((payload.len() + 8) as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(payload);
    out
}
