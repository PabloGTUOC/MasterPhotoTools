//! In-process JPEG APP1 (Exif) handling.
//!
//! Specification F13 marks EXIF preservation across a resize **mandatory**:
//! dropping the metadata block destroys the capture date that was just
//! validated, and Google Photos would then file the photograph under its upload
//! date instead of the date it was taken.
//!
//! This is done by carrying the source's APP1 segment forward verbatim and
//! patching only `PixelXDimension` / `PixelYDimension`, rather than re-deriving
//! the block. Everything the camera wrote — lens, exposure, GPS, maker notes —
//! survives untouched, and no subprocess is involved.

/// A JPEG APP1 segment carrying an Exif TIFF structure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExifBlock {
    /// The full segment: `FF E1 <len> "Exif\0\0" <tiff>`.
    bytes: Vec<u8>,
}

const EXIF_HEADER: &[u8; 6] = b"Exif\x00\x00";
const TIFF_AT: usize = 4 + 6; // FF E1 + 2-byte length + "Exif\0\0"

const TAG_EXIF_OFFSET: u16 = 0x8769;
const TAG_PIXEL_X_DIMENSION: u16 = 0xA002;
const TAG_PIXEL_Y_DIMENSION: u16 = 0xA003;

const TYPE_SHORT: u16 = 3;
const TYPE_LONG: u16 = 4;

impl ExifBlock {
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    fn tiff(&self) -> &[u8] {
        &self.bytes[TIFF_AT..]
    }

    fn tiff_mut(&mut self) -> &mut [u8] {
        &mut self.bytes[TIFF_AT..]
    }

    fn is_little_endian(&self) -> Option<bool> {
        match self.tiff().get(..2)? {
            b"II" => Some(true),
            b"MM" => Some(false),
            _ => None,
        }
    }

    /// Rewrite the recorded pixel dimensions to match a resized image.
    ///
    /// Returns true if at least one tag was updated. A file that never carried
    /// the tags is left alone — inventing them is not this function's job.
    pub fn set_pixel_dimensions(&mut self, width: u32, height: u32) -> bool {
        let Some(le) = self.is_little_endian() else {
            return false;
        };
        let tiff = self.tiff_mut();

        let Some(ifd0) = read_u32(tiff, 4, le).map(|v| v as usize) else {
            return false;
        };

        // The dimension tags live in the Exif SubIFD, reached via IFD0's pointer.
        let mut exif_ifd = None;
        visit_entries(tiff, ifd0, |tag, _ty, _count, value_at| {
            if tag == TAG_EXIF_OFFSET {
                exif_ifd = read_u32(tiff, value_at, le).map(|v| v as usize);
            }
        });

        let mut patches: Vec<(usize, u16, u32)> = Vec::new();
        for ifd in [exif_ifd, Some(ifd0)].into_iter().flatten() {
            visit_entries(tiff, ifd, |tag, ty, count, value_at| {
                if count != 1 {
                    return;
                }
                let new = match tag {
                    TAG_PIXEL_X_DIMENSION => width,
                    TAG_PIXEL_Y_DIMENSION => height,
                    _ => return,
                };
                if ty == TYPE_SHORT || ty == TYPE_LONG {
                    patches.push((value_at, ty, new));
                }
            });
            if !patches.is_empty() {
                break;
            }
        }

        let patched = !patches.is_empty();
        for (at, ty, value) in patches {
            write_inline(tiff, at, ty, value, le);
        }
        patched
    }

    /// The pixel dimensions currently recorded, if both tags are present.
    pub fn pixel_dimensions(&self) -> Option<(u32, u32)> {
        let le = self.is_little_endian()?;
        let tiff = self.tiff();
        let ifd0 = read_u32(tiff, 4, le)? as usize;

        let mut exif_ifd = None;
        visit_entries(tiff, ifd0, |tag, _ty, _count, value_at| {
            if tag == TAG_EXIF_OFFSET {
                exif_ifd = read_u32(tiff, value_at, le).map(|v| v as usize);
            }
        });

        let mut w = None;
        let mut h = None;
        for ifd in [exif_ifd, Some(ifd0)].into_iter().flatten() {
            visit_entries(tiff, ifd, |tag, ty, count, value_at| {
                if count != 1 {
                    return;
                }
                let value = match ty {
                    TYPE_SHORT => read_u16(tiff, value_at, le).map(u32::from),
                    TYPE_LONG => read_u32(tiff, value_at, le),
                    _ => None,
                };
                match tag {
                    TAG_PIXEL_X_DIMENSION => w = value,
                    TAG_PIXEL_Y_DIMENSION => h = value,
                    _ => {}
                }
            });
            if w.is_some() && h.is_some() {
                break;
            }
        }
        Some((w?, h?))
    }
}

/// Pull the Exif APP1 segment out of a JPEG, if it has one.
pub fn extract(jpeg: &[u8]) -> Option<ExifBlock> {
    for (marker, at, len) in segments(jpeg) {
        if marker != 0xE1 {
            continue;
        }
        let payload = jpeg.get(at + 4..at + 2 + len)?;
        if payload.starts_with(EXIF_HEADER) {
            return Some(ExifBlock {
                bytes: jpeg[at..at + 2 + len].to_vec(),
            });
        }
    }
    None
}

/// Write `jpeg` with `block` as its Exif APP1, replacing any it already had.
pub fn splice(jpeg: &[u8], block: &ExifBlock) -> Vec<u8> {
    let mut out = Vec::with_capacity(jpeg.len() + block.bytes.len());
    out.extend_from_slice(&jpeg[..2]); // SOI
    out.extend_from_slice(&block.bytes);

    let mut cursor = 2usize;
    for (marker, at, len) in segments(jpeg) {
        // Drop an existing Exif APP1 so the file does not end up with two.
        if marker == 0xE1 {
            if let Some(payload) = jpeg.get(at + 4..at + 2 + len) {
                if payload.starts_with(EXIF_HEADER) {
                    out.extend_from_slice(&jpeg[cursor..at]);
                    cursor = at + 2 + len;
                }
            }
        }
    }
    out.extend_from_slice(&jpeg[cursor..]);
    out
}

/// Walk the JPEG marker segments, stopping at the start of entropy-coded data.
///
/// Yields `(marker, offset_of_FF, segment_length)` where the length is the
/// 2-byte big-endian value that follows the marker.
fn segments(jpeg: &[u8]) -> Vec<(u8, usize, usize)> {
    let mut out = Vec::new();
    if jpeg.len() < 4 || jpeg[0] != 0xFF || jpeg[1] != 0xD8 {
        return out;
    }

    let mut i = 2usize;
    while i + 3 < jpeg.len() {
        if jpeg[i] != 0xFF {
            break;
        }
        let marker = jpeg[i + 1];

        // Standalone markers carry no length.
        if marker == 0xD8 || marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            i += 2;
            continue;
        }
        // Start of scan: entropy data follows, nothing more to enumerate.
        if marker == 0xDA || marker == 0xD9 {
            break;
        }

        let len = u16::from_be_bytes([jpeg[i + 2], jpeg[i + 3]]) as usize;
        if len < 2 || i + 2 + len > jpeg.len() {
            break;
        }
        out.push((marker, i, len));
        i += 2 + len;
    }
    out
}

/// Call `f(tag, type, count, offset_of_value_field)` for each entry of an IFD.
fn visit_entries<F>(tiff: &[u8], ifd_at: usize, mut f: F)
where
    F: FnMut(u16, u16, u32, usize),
{
    let le = match tiff.get(..2) {
        Some(b"II") => true,
        Some(b"MM") => false,
        _ => return,
    };
    let Some(count) = read_u16(tiff, ifd_at, le) else {
        return;
    };
    for i in 0..count as usize {
        let entry = ifd_at + 2 + i * 12;
        if entry + 12 > tiff.len() {
            return;
        }
        let (Some(tag), Some(ty), Some(n)) = (
            read_u16(tiff, entry, le),
            read_u16(tiff, entry + 2, le),
            read_u32(tiff, entry + 4, le),
        ) else {
            return;
        };
        f(tag, ty, n, entry + 8);
    }
}

fn read_u16(buf: &[u8], at: usize, le: bool) -> Option<u16> {
    let b = buf.get(at..at + 2)?;
    Some(if le {
        u16::from_le_bytes([b[0], b[1]])
    } else {
        u16::from_be_bytes([b[0], b[1]])
    })
}

fn read_u32(buf: &[u8], at: usize, le: bool) -> Option<u32> {
    let b = buf.get(at..at + 4)?;
    Some(if le {
        u32::from_le_bytes([b[0], b[1], b[2], b[3]])
    } else {
        u32::from_be_bytes([b[0], b[1], b[2], b[3]])
    })
}

fn write_inline(buf: &mut [u8], at: usize, ty: u16, value: u32, le: bool) {
    if at + 4 > buf.len() {
        return;
    }
    // The value field is always 4 bytes; a SHORT occupies the first two, and
    // the remainder must be zeroed so a stale high half is not left behind.
    let bytes = if le {
        value.to_le_bytes()
    } else {
        value.to_be_bytes()
    };
    match (ty, le) {
        (TYPE_SHORT, true) => {
            buf[at..at + 2].copy_from_slice(&(value as u16).to_le_bytes());
            buf[at + 2..at + 4].fill(0);
        }
        (TYPE_SHORT, false) => {
            buf[at..at + 2].copy_from_slice(&(value as u16).to_be_bytes());
            buf[at + 2..at + 4].fill(0);
        }
        _ => buf[at..at + 4].copy_from_slice(&bytes),
    }
}
