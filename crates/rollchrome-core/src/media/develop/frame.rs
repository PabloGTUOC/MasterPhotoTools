//! Camera aspect frame detection and cropping (DV-12).
//!
//! Reads the Olympus aspect frame from ORF metadata and crops the linear development to it,
//! matching the frame the photographer saw in the viewfinder.

use super::LinearProPhoto;

/// A bounding rectangle for the camera crop in sensor coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ByteOrder {
    LittleEndian,
    BigEndian,
}

impl ByteOrder {
    fn read_u16(self, buf: &[u8], offset: usize) -> Option<u16> {
        let bytes = buf.get(offset..offset + 2)?;
        Some(match self {
            ByteOrder::LittleEndian => u16::from_le_bytes([bytes[0], bytes[1]]),
            ByteOrder::BigEndian => u16::from_be_bytes([bytes[0], bytes[1]]),
        })
    }

    fn read_u32(self, buf: &[u8], offset: usize) -> Option<u32> {
        let bytes = buf.get(offset..offset + 4)?;
        Some(match self {
            ByteOrder::LittleEndian => u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            ByteOrder::BigEndian => u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        })
    }
}

fn find_tag_u32(buf: &[u8], ifd_offset: usize, tag: u16, order: ByteOrder) -> Option<u32> {
    let count = order.read_u16(buf, ifd_offset)? as usize;
    for i in 0..count {
        let entry_offset = ifd_offset.checked_add(2)?.checked_add(i.checked_mul(12)?)?;
        if entry_offset.checked_add(12)? > buf.len() {
            return None;
        }
        let entry_tag = order.read_u16(buf, entry_offset)?;
        if entry_tag == tag {
            return order.read_u32(buf, entry_offset + 8);
        }
    }
    None
}

/// Reads the Olympus aspect frame from RAW bytes if present.
///
/// Returns `None` for non-Olympus files or files without an aspect frame tag.
pub fn camera_frame(raw: &[u8]) -> Option<FrameRect> {
    if raw.len() < 8 {
        return None;
    }
    let file_order = match &raw[0..2] {
        b"II" => ByteOrder::LittleEndian,
        b"MM" => ByteOrder::BigEndian,
        _ => return None,
    };

    let ifd0_offset = file_order.read_u32(raw, 4)? as usize;
    let exif_ifd_offset = find_tag_u32(raw, ifd0_offset, 0x8769, file_order)? as usize;
    let mknote_offset = find_tag_u32(raw, exif_ifd_offset, 0x927C, file_order)? as usize;

    let mknote = raw.get(mknote_offset..)?;
    if mknote.get(0..8) != Some(b"OLYMPUS\0") {
        return None;
    }
    let mknote_order = match mknote.get(8..10) {
        Some(b"II") => ByteOrder::LittleEndian,
        Some(b"MM") => ByteOrder::BigEndian,
        _ => return None,
    };

    let mknote_ifd_offset = 12;
    let ip_ifd_offset = find_tag_u32(mknote, mknote_ifd_offset, 0x2040, mknote_order)? as usize;
    let aspect_offset = find_tag_u32(mknote, ip_ifd_offset, 0x1113, mknote_order)? as usize;

    let left = mknote_order.read_u16(mknote, aspect_offset)? as u32;
    let top = mknote_order.read_u16(mknote, aspect_offset + 2)? as u32;
    let right = mknote_order.read_u16(mknote, aspect_offset + 4)? as u32;
    let bottom = mknote_order.read_u16(mknote, aspect_offset + 6)? as u32;

    if right < left || bottom < top {
        return None;
    }

    let width = right.checked_sub(left)?.checked_add(1)?;
    let height = bottom.checked_sub(top)?.checked_add(1)?;

    Some(FrameRect {
        x: left,
        y: top,
        width,
        height,
    })
}

/// Crops a linear ProPhoto RGB image in place to the given frame rectangle.
///
/// If the frame does not fit inside the image, or is the whole image, this function does nothing.
pub fn crop_to_frame(linear: &mut LinearProPhoto, frame: FrameRect) {
    if frame.width == 0 || frame.height == 0 {
        return;
    }
    let end_x = match frame.x.checked_add(frame.width) {
        Some(x) => x,
        None => return,
    };
    let end_y = match frame.y.checked_add(frame.height) {
        Some(y) => y,
        None => return,
    };
    if end_x > linear.width || end_y > linear.height {
        return;
    }
    if frame.x == 0 && frame.y == 0 && frame.width == linear.width && frame.height == linear.height
    {
        return;
    }
    if linear.pixels.len() != (linear.width as usize) * (linear.height as usize) {
        return;
    }

    let mut cropped = Vec::with_capacity((frame.width * frame.height) as usize);
    for row in frame.y..end_y {
        let start = (row * linear.width + frame.x) as usize;
        let end = start + frame.width as usize;
        cropped.extend_from_slice(&linear.pixels[start..end]);
    }
    linear.pixels = cropped;
    linear.width = frame.width;
    linear.height = frame.height;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::develop::Demosaic;

    fn build_synthetic_orf(aspect: [u16; 4]) -> Vec<u8> {
        let mut buf = Vec::new();
        // Header
        buf.extend_from_slice(b"II");
        buf.extend_from_slice(&[0x52, 0x4f]); // "RO"
        buf.extend_from_slice(&8u32.to_le_bytes()); // IFD0 offset = 8

        // IFD0 at 8
        buf.extend_from_slice(&1u16.to_le_bytes()); // 1 entry
                                                    // tag 0x8769 (ExifIFD), type 4 (LONG), count 1, offset = 26
        buf.extend_from_slice(&0x8769u16.to_le_bytes());
        buf.extend_from_slice(&4u16.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&26u32.to_le_bytes()); // offset to ExifIFD
        buf.extend_from_slice(&0u32.to_le_bytes()); // next IFD = 0

        // Exif IFD at 26
        buf.extend_from_slice(&1u16.to_le_bytes()); // 1 entry
                                                    // tag 0x927C (MakerNote), type 7 (UNDEFINED), count 100, offset = 44
        buf.extend_from_slice(&0x927Cu16.to_le_bytes());
        buf.extend_from_slice(&7u16.to_le_bytes());
        buf.extend_from_slice(&100u32.to_le_bytes());
        buf.extend_from_slice(&44u32.to_le_bytes()); // offset to MakerNote
        buf.extend_from_slice(&0u32.to_le_bytes()); // next IFD = 0

        // MakerNote at 44 (offset 0 in MakerNote)
        buf.extend_from_slice(b"OLYMPUS\0"); // 8 bytes (0..8)
        buf.extend_from_slice(b"II"); // 2 bytes (8..10)
        buf.extend_from_slice(&[0, 0]); // 2 bytes (10..12)

        // MakerNote IFD at offset 12 in MakerNote
        buf.extend_from_slice(&1u16.to_le_bytes()); // 1 entry (12..14)
                                                    // tag 0x2040 (ImageProcessing), type 4 (LONG), count 1, offset = 30
        buf.extend_from_slice(&0x2040u16.to_le_bytes()); // (14..16)
        buf.extend_from_slice(&4u16.to_le_bytes()); // (16..18)
        buf.extend_from_slice(&1u32.to_le_bytes()); // (18..22)
        buf.extend_from_slice(&30u32.to_le_bytes()); // (22..26) offset to IP sub-IFD
        buf.extend_from_slice(&0u32.to_le_bytes()); // next IFD = 0 (26..30)

        // IP sub-IFD at offset 30 in MakerNote
        buf.extend_from_slice(&1u16.to_le_bytes()); // 1 entry (30..32)
                                                    // tag 0x1113 (AspectFrame), type 3 (SHORT), count 4, offset = 48
        buf.extend_from_slice(&0x1113u16.to_le_bytes()); // (32..34)
        buf.extend_from_slice(&3u16.to_le_bytes()); // (34..36)
        buf.extend_from_slice(&4u32.to_le_bytes()); // (36..40)
        buf.extend_from_slice(&48u32.to_le_bytes()); // (40..44) offset to AspectFrame values
        buf.extend_from_slice(&0u32.to_le_bytes()); // next IFD = 0 (44..48)

        // AspectFrame values at offset 48 in MakerNote
        buf.extend_from_slice(&aspect[0].to_le_bytes()); // left
        buf.extend_from_slice(&aspect[1].to_le_bytes()); // top
        buf.extend_from_slice(&aspect[2].to_le_bytes()); // right
        buf.extend_from_slice(&aspect[3].to_le_bytes()); // bottom

        buf
    }

    #[test]
    fn the_olympus_aspect_frame_is_read() {
        let raw = build_synthetic_orf([0, 152, 3647, 2583]);
        let frame = camera_frame(&raw).expect("frame found");
        assert_eq!(
            frame,
            FrameRect {
                x: 0,
                y: 152,
                width: 3648,
                height: 2432,
            }
        );
    }

    #[test]
    fn a_file_without_a_maker_note_has_no_frame() {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"II");
        buf.extend_from_slice(&[0x2a, 0x00]); // standard TIFF
        buf.extend_from_slice(&8u32.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes()); // empty IFD0
        buf.extend_from_slice(&0u32.to_le_bytes());

        assert_eq!(camera_frame(&buf), None);
    }

    #[test]
    fn a_truncated_maker_note_gives_no_frame_without_panicking() {
        let full = build_synthetic_orf([0, 152, 3647, 2583]);
        for len in 0..full.len() {
            let truncated = &full[..len];
            assert_eq!(camera_frame(truncated), None);
        }
    }

    #[test]
    fn cropping_to_a_frame_keeps_the_frame_and_ignores_one_that_does_not_fit() {
        let w = 10u32;
        let h = 10u32;
        let mut pixels = Vec::new();
        for y in 0..h {
            for x in 0..w {
                pixels.push([x as f32, y as f32, (x + y) as f32]);
            }
        }
        let original = LinearProPhoto {
            width: w,
            height: h,
            pixels: pixels.clone(),
            orientation: 1,
            demosaic: Demosaic::Bayer,
        };

        // Case 1: valid crop
        let mut linear = original.clone();
        crop_to_frame(
            &mut linear,
            FrameRect {
                x: 2,
                y: 3,
                width: 4,
                height: 4,
            },
        );
        assert_eq!(linear.width, 4);
        assert_eq!(linear.height, 4);
        assert_eq!(linear.pixels.len(), 16);
        assert_eq!(linear.pixels[0], [2.0, 3.0, 5.0]);
        assert_eq!(linear.pixels[15], [5.0, 6.0, 11.0]);

        // Case 2: frame is the whole image -> no change
        let mut linear = original.clone();
        crop_to_frame(
            &mut linear,
            FrameRect {
                x: 0,
                y: 0,
                width: w,
                height: h,
            },
        );
        assert_eq!(linear.width, w);
        assert_eq!(linear.height, h);
        assert_eq!(linear.pixels, original.pixels);

        // Case 3: frame exceeds width -> no change
        let mut linear = original.clone();
        crop_to_frame(
            &mut linear,
            FrameRect {
                x: 8,
                y: 0,
                width: 4,
                height: 5,
            },
        );
        assert_eq!(linear.width, w);
        assert_eq!(linear.height, h);
        assert_eq!(linear.pixels, original.pixels);

        // Case 4: frame exceeds height -> no change
        let mut linear = original.clone();
        crop_to_frame(
            &mut linear,
            FrameRect {
                x: 0,
                y: 8,
                width: 5,
                height: 4,
            },
        );
        assert_eq!(linear.width, w);
        assert_eq!(linear.height, h);
        assert_eq!(linear.pixels, original.pixels);

        // Case 5: zero width or height -> no change
        let mut linear = original.clone();
        crop_to_frame(
            &mut linear,
            FrameRect {
                x: 0,
                y: 0,
                width: 0,
                height: 5,
            },
        );
        assert_eq!(linear.pixels, original.pixels);
    }
}
