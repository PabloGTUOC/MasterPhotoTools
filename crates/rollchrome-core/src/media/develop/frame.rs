//! Camera aspect frame detection, distortion correction, and cropping (DV-12, DV-13).
//!
//! Reads Olympus aspect frame and camera distortion parameters from ORF metadata,
//! undistorts the linear development with the camera's own model, and crops to the frame.

use super::LinearProPhoto;
use rayon::prelude::*;

/// A bounding rectangle for the camera crop in sensor coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// The camera's embedded distortion correction coefficients (DV-13).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraDistortion {
    pub k: [f32; 3],
    pub scale: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ByteOrder {
    LittleEndian,
    BigEndian,
}

impl ByteOrder {
    fn read_u16(self, buf: &[u8], offset: usize) -> Option<u16> {
        let bytes = buf.get(offset..offset.checked_add(2)?)?;
        Some(match self {
            ByteOrder::LittleEndian => u16::from_le_bytes([bytes[0], bytes[1]]),
            ByteOrder::BigEndian => u16::from_be_bytes([bytes[0], bytes[1]]),
        })
    }

    fn read_u32(self, buf: &[u8], offset: usize) -> Option<u32> {
        let bytes = buf.get(offset..offset.checked_add(4)?)?;
        Some(match self {
            ByteOrder::LittleEndian => u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            ByteOrder::BigEndian => u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        })
    }

    fn read_f32(self, buf: &[u8], offset: usize) -> Option<f32> {
        let bytes = buf.get(offset..offset.checked_add(4)?)?;
        Some(match self {
            ByteOrder::LittleEndian => f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            ByteOrder::BigEndian => f32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        })
    }
}

fn find_tag_entry(
    buf: &[u8],
    ifd_offset: usize,
    tag: u16,
    order: ByteOrder,
) -> Option<(u16, u32, u32)> {
    let count = order.read_u16(buf, ifd_offset)? as usize;
    for i in 0..count {
        let entry_offset = ifd_offset.checked_add(2)?.checked_add(i.checked_mul(12)?)?;
        if entry_offset.checked_add(12)? > buf.len() {
            return None;
        }
        let entry_tag = order.read_u16(buf, entry_offset)?;
        if entry_tag == tag {
            let typ = order.read_u16(buf, entry_offset + 2)?;
            let cnt = order.read_u32(buf, entry_offset + 4)?;
            let val = order.read_u32(buf, entry_offset + 8)?;
            return Some((typ, cnt, val));
        }
    }
    None
}

fn find_tag_u32(buf: &[u8], ifd_offset: usize, tag: u16, order: ByteOrder) -> Option<u32> {
    find_tag_entry(buf, ifd_offset, tag, order).map(|(_, _, val)| val)
}

fn olympus_image_processing_ifd(raw: &[u8]) -> Option<(&[u8], ByteOrder, usize)> {
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
    Some((mknote, mknote_order, ip_ifd_offset))
}

/// Reads the Olympus aspect frame from RAW bytes if present.
///
/// Returns `None` for non-Olympus files or files without an aspect frame tag.
pub fn camera_frame(raw: &[u8]) -> Option<FrameRect> {
    let (mknote, mknote_order, ip_ifd_offset) = olympus_image_processing_ifd(raw)?;
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

/// Reads the Olympus camera distortion correction from RAW bytes if present (DV-13).
///
/// Tag 0x150A in the ImageProcessing sub-IFD contains [k1, k2, k3, scale] as four 32-bit floats.
/// Returns `None` if the tag is missing, wrong type/count, non-finite, scale <= 0, or identity
/// (k == [0, 0, 0] and scale == 1.0).
pub fn camera_distortion(raw: &[u8]) -> Option<CameraDistortion> {
    let (mknote, mknote_order, ip_ifd_offset) = olympus_image_processing_ifd(raw)?;
    let (entry_type, count, val_offset) =
        find_tag_entry(mknote, ip_ifd_offset, 0x150A, mknote_order)?;
    if entry_type != 11 || count != 4 {
        return None;
    }
    let offset = val_offset as usize;
    let k0 = mknote_order.read_f32(mknote, offset)?;
    let k1 = mknote_order.read_f32(mknote, offset.checked_add(4)?)?;
    let k2 = mknote_order.read_f32(mknote, offset.checked_add(8)?)?;
    let scale = mknote_order.read_f32(mknote, offset.checked_add(12)?)?;

    if !k0.is_finite() || !k1.is_finite() || !k2.is_finite() || !scale.is_finite() {
        return None;
    }
    if scale <= 0.0 {
        return None;
    }
    if k0 == 0.0 && k1 == 0.0 && k2 == 0.0 && scale == 1.0 {
        return None;
    }

    Some(CameraDistortion {
        k: [k0, k1, k2],
        scale,
    })
}

/// Bilinear sample of all channels at a pixel-centre coordinate, clamped to the image boundary.
#[inline]
fn sample_bilinear(pixels: &[[f32; 3]], w: usize, h: usize, x: f32, y: f32) -> [f32; 3] {
    let x = x.clamp(0.0, (w - 1) as f32);
    let y = y.clamp(0.0, (h - 1) as f32);
    let x0 = x.floor() as usize;
    let y0 = y.floor() as usize;
    let x1 = (x0 + 1).min(w - 1);
    let y1 = (y0 + 1).min(h - 1);
    let fx = x - x0 as f32;
    let fy = y - y0 as f32;
    let p00 = pixels[y0 * w + x0];
    let p10 = pixels[y0 * w + x1];
    let p01 = pixels[y1 * w + x0];
    let p11 = pixels[y1 * w + x1];
    let mut out = [0.0f32; 3];
    for ch in 0..3 {
        let top = p00[ch] + (p10[ch] - p00[ch]) * fx;
        let bottom = p01[ch] + (p11[ch] - p01[ch]) * fx;
        out[ch] = top + (bottom - top) * fy;
    }
    out
}

/// Undistorts a linear ProPhoto RGB image in place using the camera's geometric model (DV-13).
///
/// Output dimensions remain unchanged. Rows are parallelised with rayon.
/// If pixel count does not match width * height, does nothing.
pub fn undistort(linear: &mut LinearProPhoto, d: CameraDistortion) {
    let (w, h) = (linear.width as usize, linear.height as usize);
    if w == 0 || h == 0 || linear.pixels.len() != w * h {
        return;
    }
    let w_f = w as f32;
    let h_f = h as f32;
    let cx = (w_f - 1.0) * 0.5;
    let cy = (h_f - 1.0) * 0.5;
    let half_diag = (w_f * w_f + h_f * h_f).sqrt() * 0.5;
    if half_diag == 0.0 {
        return;
    }

    let mut out = vec![[0.0f32; 3]; w * h];
    out.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        let vy = y as f32 - cy;
        for (x, px) in row.iter_mut().enumerate() {
            let vx = x as f32 - cx;
            let v_len = (vx * vx + vy * vy).sqrt();
            let r = v_len / half_diag;
            let r2 = r * r;
            let r4 = r2 * r2;
            let r6 = r4 * r2;
            let poly = 1.0 + d.k[0] * r2 + d.k[1] * r4 + d.k[2] * r6;
            let f = d.scale * poly;
            *px = sample_bilinear(&linear.pixels, w, h, cx + vx * f, cy + vy * f);
        }
    });
    linear.pixels = out;
}

/// Applies camera geometry corrections to a linear development in place (DV-13):
/// undistorts according to the camera's distortion parameters if present,
/// then crops to the aspect frame if present.
pub fn apply_camera_geometry(linear: &mut LinearProPhoto, raw: &[u8]) {
    if let Some(distortion) = camera_distortion(raw) {
        undistort(linear, distortion);
    }
    if let Some(frame) = camera_frame(raw) {
        crop_to_frame(linear, frame);
    }
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
#[allow(clippy::excessive_precision)]
mod tests {
    use super::*;
    use crate::media::develop::Demosaic;

    fn build_synthetic_orf_full(
        aspect: Option<[u16; 4]>,
        distortion: Option<(u16, u32, &[u8])>,
    ) -> Vec<u8> {
        let mut entries: Vec<(u16, u16, u32, Vec<u8>)> = Vec::new();
        if let Some(a) = aspect {
            let mut data = Vec::with_capacity(8);
            data.extend_from_slice(&a[0].to_le_bytes());
            data.extend_from_slice(&a[1].to_le_bytes());
            data.extend_from_slice(&a[2].to_le_bytes());
            data.extend_from_slice(&a[3].to_le_bytes());
            entries.push((0x1113, 3, 4, data));
        }
        if let Some((typ, count, d)) = distortion {
            entries.push((0x150A, typ, count, d.to_vec()));
        }

        let mut buf = Vec::new();
        // Header
        buf.extend_from_slice(b"II");
        buf.extend_from_slice(&[0x52, 0x4f]); // "RO"
        buf.extend_from_slice(&8u32.to_le_bytes()); // IFD0 offset = 8

        // IFD0 at 8
        buf.extend_from_slice(&1u16.to_le_bytes()); // 1 entry
        buf.extend_from_slice(&0x8769u16.to_le_bytes());
        buf.extend_from_slice(&4u16.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&26u32.to_le_bytes()); // offset to ExifIFD
        buf.extend_from_slice(&0u32.to_le_bytes()); // next IFD = 0

        // Exif IFD at 26
        buf.extend_from_slice(&1u16.to_le_bytes()); // 1 entry
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
        buf.extend_from_slice(&0x2040u16.to_le_bytes()); // (14..16)
        buf.extend_from_slice(&4u16.to_le_bytes()); // (16..18)
        buf.extend_from_slice(&1u32.to_le_bytes()); // (18..22)
        buf.extend_from_slice(&30u32.to_le_bytes()); // (22..26) offset to IP sub-IFD
        buf.extend_from_slice(&0u32.to_le_bytes()); // next IFD = 0 (26..30)

        // IP sub-IFD at offset 30 in MakerNote
        let num_entries = entries.len() as u16;
        buf.extend_from_slice(&num_entries.to_le_bytes());

        let mut cur_data_offset = 30 + 2 + (entries.len() as u32) * 12 + 4;
        for (tag, typ, count, data) in &entries {
            buf.extend_from_slice(&tag.to_le_bytes());
            buf.extend_from_slice(&typ.to_le_bytes());
            buf.extend_from_slice(&count.to_le_bytes());
            buf.extend_from_slice(&cur_data_offset.to_le_bytes());
            cur_data_offset += data.len() as u32;
        }
        buf.extend_from_slice(&0u32.to_le_bytes()); // next IFD = 0

        for (_, _, _, data) in &entries {
            buf.extend_from_slice(data);
        }

        buf
    }

    fn build_synthetic_orf(aspect: [u16; 4]) -> Vec<u8> {
        build_synthetic_orf_full(Some(aspect), None)
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

    #[test]
    fn the_olympus_distortion_is_read() {
        let bytes = [
            0x39, 0xf9, 0x5a, 0xbd, 0x87, 0xfb, 0x75, 0x3c, 0xe6, 0xb6, 0x8c, 0x3b, 0x00, 0x40,
            0x81, 0x3f,
        ];
        let raw = build_synthetic_orf_full(None, Some((11, 4, &bytes)));
        let d = camera_distortion(&raw).expect("distortion found");
        assert!((d.k[0] - (-0.053460333)).abs() < 1e-7);
        assert!((d.k[1] - 0.015013582).abs() < 1e-7);
        assert!((d.k[2] - 0.0042942641).abs() < 1e-7);
        assert!((d.scale - 1.009765625).abs() < 1e-7);
    }

    #[test]
    fn a_file_without_the_distortion_tag_has_none() {
        let raw = build_synthetic_orf([0, 152, 3647, 2583]);
        assert_eq!(camera_distortion(&raw), None);

        let mut not_olympus = Vec::new();
        not_olympus.extend_from_slice(b"II");
        not_olympus.extend_from_slice(&[0x2a, 0x00]);
        not_olympus.extend_from_slice(&8u32.to_le_bytes());
        not_olympus.extend_from_slice(&0u16.to_le_bytes());
        not_olympus.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(camera_distortion(&not_olympus), None);
    }

    #[test]
    fn a_distortion_tag_of_the_wrong_type_or_count_is_ignored() {
        // Wrong type: type 5 (RATIONAL)
        let dummy = [0u8; 16];
        let raw_wrong_type = build_synthetic_orf_full(None, Some((5, 4, &dummy)));
        assert_eq!(camera_distortion(&raw_wrong_type), None);

        // Wrong count: type 11 with count 3
        let raw_wrong_count = build_synthetic_orf_full(None, Some((11, 3, &[0u8; 12])));
        assert_eq!(camera_distortion(&raw_wrong_count), None);

        // k = [0, 0, 0], scale = 1.0 -> None (no correction)
        let mut no_corr = Vec::new();
        for &f in &[0.0f32, 0.0, 0.0, 1.0] {
            no_corr.extend_from_slice(&f.to_le_bytes());
        }
        let raw_no_corr = build_synthetic_orf_full(None, Some((11, 4, &no_corr)));
        assert_eq!(camera_distortion(&raw_no_corr), None);

        // k = [0, 0, 0], scale = 0.9912109375 -> Some
        let mut scale_only = Vec::new();
        for &f in &[0.0f32, 0.0, 0.0, 0.9912109375] {
            scale_only.extend_from_slice(&f.to_le_bytes());
        }
        let raw_scale_only = build_synthetic_orf_full(None, Some((11, 4, &scale_only)));
        let d = camera_distortion(&raw_scale_only).expect("scale only found");
        assert_eq!(d.k, [0.0, 0.0, 0.0]);
        assert_eq!(d.scale, 0.9912109375);
    }

    #[test]
    fn a_truncated_distortion_tag_gives_none_without_panicking() {
        let bytes = [
            0x39, 0xf9, 0x5a, 0xbd, 0x87, 0xfb, 0x75, 0x3c, 0xe6, 0xb6, 0x8c, 0x3b, 0x00, 0x40,
            0x81, 0x3f,
        ];
        let full = build_synthetic_orf_full(None, Some((11, 4, &bytes)));
        for len in 0..full.len() {
            assert_eq!(camera_distortion(&full[..len]), None);
        }
    }

    #[test]
    fn undistortion_samples_where_the_camera_says() {
        let w = 400u32;
        let h = 300u32;
        let mut pixels = Vec::with_capacity((w * h) as usize);
        for y in 0..h {
            for x in 0..w {
                pixels.push([x as f32, y as f32, 0.0]);
            }
        }
        let mut linear = LinearProPhoto {
            width: w,
            height: h,
            pixels,
            orientation: 1,
            demosaic: Demosaic::Bayer,
        };
        let d = CameraDistortion {
            k: [-0.0534603335, 0.0150135821, 0.0042942641],
            scale: 1.009765625,
        };
        undistort(&mut linear, d);
        assert_eq!(linear.width, 400);
        assert_eq!(linear.height, 300);

        let px = |x: usize, y: usize| linear.pixels[y * 400 + x];
        let p_0_0 = px(0, 0);
        assert!((p_0_0[0] - 4.920).abs() < 0.01, "got {:?}", p_0_0);
        assert!((p_0_0[1] - 3.687).abs() < 0.01, "got {:?}", p_0_0);

        let p_399_299 = px(399, 299);
        assert!((p_399_299[0] - 394.080).abs() < 0.01, "got {:?}", p_399_299);
        assert!((p_399_299[1] - 295.313).abs() < 0.01, "got {:?}", p_399_299);

        let p_0_150 = px(0, 150);
        assert!((p_0_150[0] - 3.460).abs() < 0.01, "got {:?}", p_0_150);
        assert!((p_0_150[1] - 149.991).abs() < 0.01, "got {:?}", p_0_150);

        let p_300_50 = px(300, 50);
        assert!((p_300_50[0] - 299.416).abs() < 0.01, "got {:?}", p_300_50);
        assert!((p_300_50[1] - 50.579).abs() < 0.01, "got {:?}", p_300_50);
    }

    #[test]
    fn a_scale_alone_magnifies_evenly() {
        let w = 400u32;
        let h = 300u32;
        let mut pixels = Vec::with_capacity((w * h) as usize);
        for y in 0..h {
            for x in 0..w {
                pixels.push([x as f32, y as f32, 0.0]);
            }
        }
        let mut linear = LinearProPhoto {
            width: w,
            height: h,
            pixels,
            orientation: 1,
            demosaic: Demosaic::Bayer,
        };
        let d = CameraDistortion {
            k: [0.0, 0.0, 0.0],
            scale: 0.9912109375,
        };
        undistort(&mut linear, d);

        let px = |x: usize, y: usize| linear.pixels[y * 400 + x];
        let p_0_0 = px(0, 0);
        assert!((p_0_0[0] - 1.753).abs() < 0.01, "got {:?}", p_0_0);
        assert!((p_0_0[1] - 1.314).abs() < 0.01, "got {:?}", p_0_0);

        let p_100_200 = px(100, 200);
        assert!((p_100_200[0] - 100.875).abs() < 0.01, "got {:?}", p_100_200);
        assert!((p_100_200[1] - 199.556).abs() < 0.01, "got {:?}", p_100_200);
    }

    #[test]
    fn the_camera_geometry_corrects_before_it_crops() {
        let w = 400u32;
        let h = 300u32;
        let mut pixels = Vec::with_capacity((w * h) as usize);
        for y in 0..h {
            for x in 0..w {
                pixels.push([x as f32, y as f32, 0.0]);
            }
        }
        let mut linear = LinearProPhoto {
            width: w,
            height: h,
            pixels,
            orientation: 1,
            demosaic: Demosaic::Bayer,
        };
        let bytes = [
            0x39, 0xf9, 0x5a, 0xbd, 0x87, 0xfb, 0x75, 0x3c, 0xe6, 0xb6, 0x8c, 0x3b, 0x00, 0x40,
            0x81, 0x3f,
        ];
        let raw = build_synthetic_orf_full(Some([0, 50, 399, 249]), Some((11, 4, &bytes)));
        apply_camera_geometry(&mut linear, &raw);

        assert_eq!(linear.width, 400);
        assert_eq!(linear.height, 200);

        let px = |x: usize, y: usize| linear.pixels[y * 400 + x];
        let p_0_0 = px(0, 0);
        assert!((p_0_0[0] - 4.268).abs() < 0.01, "got {:?}", p_0_0);
        assert!((p_0_0[1] - 52.129).abs() < 0.01, "got {:?}", p_0_0);

        let p_399_199 = px(399, 199);
        assert!((p_399_199[0] - 394.732).abs() < 0.01, "got {:?}", p_399_199);
        assert!((p_399_199[1] - 246.871).abs() < 0.01, "got {:?}", p_399_199);
    }
}
