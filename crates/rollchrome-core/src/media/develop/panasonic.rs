//! Panasonic RW2 geometry: orientation and lens distortion correction (DV-15).
//!
//! RW2 files are TIFF containers whose header has `55 00` (`II U \0`) instead of `2A 00`.
//! Panasonic cameras store EXIF orientation in IFD0 tag `0x0112` and lens distortion parameters
//! in IFD0 tag `0x0119` (DistortionInfo: 16 little-endian `i16` values with a checksum).

use super::frame::{find_tag_entry, sample_bilinear, ByteOrder};
use super::LinearProPhoto;
use rayon::prelude::*;

/// Panasonic embedded distortion correction parameters (DV-15).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanasonicDistortion {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub scale: f32,
    pub on: bool,
}

/// What Panasonic's black-level tags understate the sensor's black by, in raw units.
///
/// The LX100 writes 128 in `0x1C`–`0x1E`, yet in an ISO 400 frame not one pixel in ten thousand
/// reads below 143. With 128 every channel keeps 15 units of black, which the camera's 2.8× red gain
/// turns into a magenta cast: red/green 1.087 on grey paving where the camera's JPEG has 1.000, and
/// 1.011 at 143 (DV-16). rawspeed, darktable's decoder, adds the same 15 to every Panasonic body.
pub const RW2_BLACK_BIAS: u16 = 15;

/// The sensor's black level for red, green and blue, from an RW2's IFD0 tags `0x1C`, `0x1D` and
/// `0x1E` plus [`RW2_BLACK_BIAS`].
///
/// rawler reads the same tags but without the bias, and lays them out red, green, green, blue
/// whatever the CFA, so a BGGR sensor gets red's value on its blue pixels. `None` unless the header
/// is `IIU\0` and all three tags are SHORTs (type 3, count 1).
pub fn rw2_black_levels(raw: &[u8]) -> Option<[u16; 3]> {
    if raw.len() < 8 || &raw[0..4] != b"IIU\0" {
        return None;
    }
    let order = ByteOrder::LittleEndian;
    let ifd0_offset = order.read_u32(raw, 4)? as usize;
    let mut levels = [0u16; 3];
    for (level, tag) in levels.iter_mut().zip([0x1C, 0x1D, 0x1E]) {
        let (typ, cnt, val) = find_tag_entry(raw, ifd0_offset, tag, order)?;
        if typ != 3 || cnt != 1 {
            return None;
        }
        *level = ((val & 0xFFFF) as u16).checked_add(RW2_BLACK_BIAS)?;
    }
    Some(levels)
}

/// Red, green and blue black levels laid out by a 2×2 CFA's positions, row by row, the order rawler's
/// `BlackLevel` and its scaling read them in.
pub fn black_by_cfa_position(rgb: [u16; 3], cfa: &rawler::CFA) -> [u16; 4] {
    std::array::from_fn(|i| rgb[cfa.color_at(i / 2, i % 2).min(2)])
}

/// Reads the EXIF orientation from an RW2 file's IFD0 tag `0x0112`.
///
/// Returns `None` unless the header is `IIU\0`, the tag exists as a SHORT (type 3, count 1),
/// and the value is in 1..=8.
pub fn rw2_orientation(raw: &[u8]) -> Option<u16> {
    if raw.len() < 8 || &raw[0..4] != b"IIU\0" {
        return None;
    }
    let order = ByteOrder::LittleEndian;
    let ifd0_offset = order.read_u32(raw, 4)? as usize;
    let (typ, cnt, val) = find_tag_entry(raw, ifd0_offset, 0x0112, order)?;
    if typ != 3 || cnt != 1 {
        return None;
    }
    let ori = (val & 0xFFFF) as u16;
    if (1..=8).contains(&ori) {
        Some(ori)
    } else {
        None
    }
}

/// Computes the ExifTool / Panasonic 32-byte checksum.
///
/// `checksum(start, n, inc) = fold over i in 0..n: c = (73 * c + d[start + i * inc]) mod 0xFFEF, from c = 0`
fn panasonic_checksum(d: &[u8; 32], start: usize, n: usize, inc: usize) -> u16 {
    let mut c: u32 = 0;
    for i in 0..n {
        let byte = d[start + i * inc] as u32;
        c = (73 * c + byte) % 0xFFEF;
    }
    c as u16
}

#[inline]
fn u16le(d: &[u8; 32], offset: usize) -> u16 {
    u16::from_le_bytes([d[offset], d[offset + 1]])
}

/// Reads Panasonic lens distortion parameters from an RW2 file's IFD0 tag `0x0119`.
///
/// Returns `None` unless the header is `IIU\0`, IFD0 contains tag `0x0119` (type 7 UNDEFINED,
/// count 32), the 32 bytes are in bounds, the checksum is valid, and `1 + v[5]/32768 > 0`.
///
/// Returns `Some(PanasonicDistortion)` even when `on` is false, indicating the lens profile.
pub fn panasonic_distortion(raw: &[u8]) -> Option<PanasonicDistortion> {
    if raw.len() < 8 || &raw[0..4] != b"IIU\0" {
        return None;
    }
    let order = ByteOrder::LittleEndian;
    let ifd0_offset = order.read_u32(raw, 4)? as usize;
    let (typ, cnt, val) = find_tag_entry(raw, ifd0_offset, 0x0119, order)?;
    if typ != 7 || cnt != 32 {
        return None;
    }
    let offset = val as usize;
    let d_slice = raw.get(offset..offset.checked_add(32)?)?;
    let mut d = [0u8; 32];
    d.copy_from_slice(d_slice);

    // Verify checksum
    let valid = panasonic_checksum(&d, 4, 12, 1)
        ^ u16le(&d, 2)
        ^ panasonic_checksum(&d, 16, 12, 1)
        ^ u16le(&d, 28)
        ^ panasonic_checksum(&d, 2, 14, 2)
        ^ u16le(&d, 0)
        ^ panasonic_checksum(&d, 3, 14, 2)
        ^ u16le(&d, 30)
        == 0;
    if !valid {
        return None;
    }

    let mut v = [0i16; 16];
    for i in 0..16 {
        v[i] = i16::from_le_bytes([d[2 * i], d[2 * i + 1]]);
    }

    let denom = 1.0f32 + (v[5] as f32 / 32768.0);
    if denom <= 0.0 {
        return None;
    }
    let scale = 1.0f32 / denom;
    let b = v[4] as f32 / 32768.0;
    let on = (v[7] & 0x0F) == 1;
    let a = v[8] as f32 / 32768.0;
    let c = v[11] as f32 / 32768.0;

    Some(PanasonicDistortion { a, b, c, scale, on })
}

/// Undistorts a linear ProPhoto RGB image in place using the Panasonic camera model (DV-15).
///
/// Uses the inverse equation: for each output pixel with normalized radius `ru = |v| / R`,
/// finds `rd >= 0` such that `scale * (rd + a*rd^3 + b*rd^5 + c*rd^7) = ru`, then samples
/// the source image at `c + v * (rd / ru)` (bilinear, clamped).
///
/// Does nothing when `pixels.len() != width * height`, when `!d.on`, or when
/// `a == 0 && b == 0 && c == 0 && scale == 1`.
/// Also does nothing if the model derivative `f'(r)` is <= 0 anywhere on [0, 1.2] at 0.01 steps.
pub fn undistort_panasonic(linear: &mut LinearProPhoto, d: PanasonicDistortion) {
    let (w, h) = (linear.width as usize, linear.height as usize);
    if w == 0 || h == 0 || linear.pixels.len() != w * h {
        return;
    }
    if !d.on {
        return;
    }
    if d.a == 0.0 && d.b == 0.0 && d.c == 0.0 && d.scale == 1.0 {
        return;
    }

    // Monotonicity check on [0, 1.2] at 0.01 steps
    for step in 0..=120 {
        let r = step as f32 * 0.01;
        let r2 = r * r;
        let r4 = r2 * r2;
        let r6 = r4 * r2;
        let f_prime = d.scale * (1.0 + 3.0 * d.a * r2 + 5.0 * d.b * r4 + 7.0 * d.c * r6);
        if f_prime <= 0.0 {
            return;
        }
    }

    let w_f = w as f32;
    let h_f = h as f32;
    let cx = (w_f - 1.0) * 0.5;
    let cy = (h_f - 1.0) * 0.5;
    let half_diag = (w_f * w_f + h_f * h_f).sqrt() * 0.5;
    if half_diag == 0.0 {
        return;
    }

    let scale_f64 = d.scale as f64;
    let a_f64 = d.a as f64;
    let b_f64 = d.b as f64;
    let c_f64 = d.c as f64;

    let mut out = vec![[0.0f32; 3]; w * h];
    out.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        let vy = y as f32 - cy;
        for (x, px) in row.iter_mut().enumerate() {
            let vx = x as f32 - cx;
            let v_len = (vx * vx + vy * vy).sqrt();
            if v_len == 0.0 {
                *px = sample_bilinear(&linear.pixels, w, h, cx, cy);
                continue;
            }
            let ru = (v_len / half_diag) as f64;

            // Solve scale * (rd + a*rd^3 + b*rd^5 + c*rd^7) = ru with Newton's method
            let mut rd = ru / scale_f64;
            for _ in 0..8 {
                let rd2 = rd * rd;
                let rd3 = rd2 * rd;
                let rd4 = rd2 * rd2;
                let rd5 = rd4 * rd;
                let rd6 = rd4 * rd2;
                let rd7 = rd6 * rd;
                let f_rd = scale_f64 * (rd + a_f64 * rd3 + b_f64 * rd5 + c_f64 * rd7);
                let f_prime =
                    scale_f64 * (1.0 + 3.0 * a_f64 * rd2 + 5.0 * b_f64 * rd4 + 7.0 * c_f64 * rd6);
                let step = (f_rd - ru) / f_prime;
                rd -= step;
                if step.abs() < 1e-7 {
                    break;
                }
            }

            let ratio = (rd / ru) as f32;
            let sample_x = cx + vx * ratio;
            let sample_y = cy + vy * ratio;
            *px = sample_bilinear(&linear.pixels, w, h, sample_x, sample_y);
        }
    });
    linear.pixels = out;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::develop::Demosaic;

    fn make_synthetic_rw2(
        header: &[u8; 4],
        entries: &[(u16, u16, u32, u32)],
        extra: &[u8],
    ) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(header);
        buf.extend_from_slice(&8u32.to_le_bytes()); // IFD0 at offset 8
        buf.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        for &(tag, typ, cnt, val) in entries {
            buf.extend_from_slice(&tag.to_le_bytes());
            buf.extend_from_slice(&typ.to_le_bytes());
            buf.extend_from_slice(&cnt.to_le_bytes());
            buf.extend_from_slice(&val.to_le_bytes());
        }
        buf.extend_from_slice(&0u32.to_le_bytes()); // next IFD = 0
        buf.extend_from_slice(extra);
        buf
    }

    const LX100_10_9MM_TAG: [u8; 32] = [
        0x1b, 0xa5, 0xa5, 0xbb, 0x43, 0x01, 0x00, 0x00, 0x0e, 0x07, 0x00, 0x00, 0xb3, 0x01, 0x01,
        0x00, 0x1e, 0x18, 0xa9, 0x00, 0x34, 0x00, 0x59, 0xf9, 0x0b, 0x0a, 0x9d, 0x02, 0x19, 0x4a,
        0xec, 0x17,
    ];

    const LX100_34MM_TAG: [u8; 32] = [
        0x93, 0x86, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x34, 0x00, 0x00, 0x00, 0x0b, 0x0a, 0x00, 0x00, 0x29, 0xf3,
        0x1d, 0xd1,
    ];

    #[test]
    fn the_panasonic_distortion_is_read() {
        // IFD0 at 8, 1 entry (12 bytes) + 4-byte next = offset 8 + 2 + 12 + 4 = 26
        let raw = make_synthetic_rw2(b"IIU\0", &[(0x0119, 7, 32, 26)], &LX100_10_9MM_TAG);
        let d = panasonic_distortion(&raw).expect("valid distortion tag");
        assert!((d.a - 6174.0 / 32768.0).abs() < 1e-7);
        assert!((d.b - 1806.0 / 32768.0).abs() < 1e-7);
        assert!((d.c - (-1703.0 / 32768.0)).abs() < 1e-7);
        assert!((d.scale - 1.0).abs() < 1e-7);
        assert!(d.on);
    }

    #[test]
    fn a_switched_off_or_damaged_distortion_is_handled() {
        let raw_34mm = make_synthetic_rw2(b"IIU\0", &[(0x0119, 7, 32, 26)], &LX100_34MM_TAG);
        let d = panasonic_distortion(&raw_34mm).expect("valid distortion tag");
        assert!(!d.on);

        let mut damaged_tag = LX100_10_9MM_TAG;
        damaged_tag[16] ^= 1;
        let raw_corrupted = make_synthetic_rw2(b"IIU\0", &[(0x0119, 7, 32, 26)], &damaged_tag);
        assert_eq!(panasonic_distortion(&raw_corrupted), None);

        let raw_bad_cnt = make_synthetic_rw2(b"IIU\0", &[(0x0119, 7, 31, 26)], &LX100_10_9MM_TAG);
        assert_eq!(panasonic_distortion(&raw_bad_cnt), None);

        let raw_bad_typ = make_synthetic_rw2(b"IIU\0", &[(0x0119, 1, 32, 26)], &LX100_10_9MM_TAG);
        assert_eq!(panasonic_distortion(&raw_bad_typ), None);
    }

    #[test]
    fn a_file_that_is_not_rw2_has_no_panasonic_geometry() {
        let tiff = make_synthetic_rw2(
            b"II*\0",
            &[(0x0112, 3, 1, 6), (0x0119, 7, 32, 38)],
            &LX100_10_9MM_TAG,
        );
        assert_eq!(rw2_orientation(&tiff), None);
        assert_eq!(panasonic_distortion(&tiff), None);

        let raw = make_synthetic_rw2(b"IIU\0", &[(0x0119, 7, 32, 26)], &LX100_10_9MM_TAG);
        for len in 0..raw.len() {
            let truncated = &raw[..len];
            let _ = rw2_orientation(truncated);
            let _ = panasonic_distortion(truncated);
        }
    }

    #[test]
    fn the_rw2_black_level_carries_panasonic_s_bias() {
        let black = |r: u32, g: u32, b: u32| [(0x1C, 3, 1, r), (0x1D, 3, 1, g), (0x1E, 3, 1, b)];
        let lx100 = make_synthetic_rw2(b"IIU\0", &black(128, 128, 128), &[]);
        assert_eq!(rw2_black_levels(&lx100), Some([143, 143, 143]));

        let uneven = make_synthetic_rw2(b"IIU\0", &black(120, 130, 140), &[]);
        assert_eq!(rw2_black_levels(&uneven), Some([135, 145, 155]));

        let tiff = make_synthetic_rw2(b"II*\0", &black(128, 128, 128), &[]);
        assert_eq!(rw2_black_levels(&tiff), None);

        let missing_blue = make_synthetic_rw2(b"IIU\0", &black(128, 128, 128)[..2], &[]);
        assert_eq!(rw2_black_levels(&missing_blue), None);

        let long = make_synthetic_rw2(
            b"IIU\0",
            &[(0x1C, 4, 1, 128), (0x1D, 3, 1, 128), (0x1E, 3, 1, 128)],
            &[],
        );
        assert_eq!(rw2_black_levels(&long), None);
    }

    #[test]
    fn the_black_level_follows_the_sensor_s_cfa() {
        let rgb = [10, 20, 30];
        assert_eq!(
            black_by_cfa_position(rgb, &rawler::CFA::new("BGGR")),
            [30, 20, 20, 10]
        );
        assert_eq!(
            black_by_cfa_position(rgb, &rawler::CFA::new("RGGB")),
            [10, 20, 20, 30]
        );
    }

    #[test]
    fn the_rw2_orientation_is_read() {
        let raw6 = make_synthetic_rw2(b"IIU\0", &[(0x0112, 3, 1, 6)], &[]);
        assert_eq!(rw2_orientation(&raw6), Some(6));

        let raw0 = make_synthetic_rw2(b"IIU\0", &[(0x0112, 3, 1, 0)], &[]);
        assert_eq!(rw2_orientation(&raw0), None);

        let raw9 = make_synthetic_rw2(b"IIU\0", &[(0x0112, 3, 1, 9)], &[]);
        assert_eq!(rw2_orientation(&raw9), None);

        let tiff6 = make_synthetic_rw2(b"II*\0", &[(0x0112, 3, 1, 6)], &[]);
        assert_eq!(rw2_orientation(&tiff6), None);
    }

    #[test]
    fn undistortion_follows_the_panasonic_model() {
        let mut pixels = Vec::with_capacity(400 * 300);
        for y in 0..300 {
            for x in 0..400 {
                pixels.push([x as f32, y as f32, 0.0]);
            }
        }
        let mut linear = LinearProPhoto {
            width: 400,
            height: 300,
            pixels: pixels.clone(),
            orientation: 1,
            demosaic: Demosaic::Bayer,
        };

        let d = PanasonicDistortion {
            a: 6174.0 / 32768.0,
            b: 1806.0 / 32768.0,
            c: -1703.0 / 32768.0,
            scale: 1.0,
            on: true,
        };
        undistort_panasonic(&mut linear, d);

        assert_eq!(linear.width, 400);
        assert_eq!(linear.height, 300);

        let check = |l: &LinearProPhoto, x: usize, y: usize, exp_r: f32, exp_g: f32| {
            let p = l.pixels[y * 400 + x];
            assert!(
                (p[0] - exp_r).abs() < 0.01,
                "at ({x}, {y}) R: got {}, expected {}",
                p[0],
                exp_r
            );
            assert!(
                (p[1] - exp_g).abs() < 0.01,
                "at ({x}, {y}) G: got {}, expected {}",
                p[1],
                exp_g
            );
        };

        check(&linear, 0, 0, 26.112, 19.567);
        check(&linear, 399, 299, 372.888, 279.433);
        check(&linear, 0, 150, 19.078, 149.952);
        check(&linear, 300, 50, 294.559, 55.387);

        // With on: false, image is unchanged
        let mut unchanged = LinearProPhoto {
            width: 400,
            height: 300,
            pixels: pixels.clone(),
            orientation: 1,
            demosaic: Demosaic::Bayer,
        };
        let mut d_off = d;
        d_off.on = false;
        undistort_panasonic(&mut unchanged, d_off);
        assert_eq!(unchanged.pixels, pixels);
    }

    #[test]
    fn the_distortion_scale_applies() {
        let mut pixels = Vec::with_capacity(400 * 300);
        for y in 0..300 {
            for x in 0..400 {
                pixels.push([x as f32, y as f32, 0.0]);
            }
        }
        let mut linear = LinearProPhoto {
            width: 400,
            height: 300,
            pixels,
            orientation: 1,
            demosaic: Demosaic::Bayer,
        };

        let d = PanasonicDistortion {
            a: 6174.0 / 32768.0,
            b: 1806.0 / 32768.0,
            c: -1703.0 / 32768.0,
            scale: 1.0 / (1.0 - 2048.0 / 32768.0),
            on: true,
        };
        undistort_panasonic(&mut linear, d);

        let check = |l: &LinearProPhoto, x: usize, y: usize, exp_r: f32, exp_g: f32| {
            let p = l.pixels[y * 400 + x];
            assert!(
                (p[0] - exp_r).abs() < 0.01,
                "at ({x}, {y}) R: got {}, expected {}",
                p[0],
                exp_r
            );
            assert!(
                (p[1] - exp_g).abs() < 0.01,
                "at ({x}, {y}) G: got {}, expected {}",
                p[1],
                exp_g
            );
        };

        check(&linear, 0, 0, 34.947, 26.188);
        check(&linear, 399, 299, 364.053, 272.812);
        check(&linear, 300, 50, 289.171, 60.721);
    }
}
