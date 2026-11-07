//! Handmade DNG writer for tests and benchmarks (DV-1).
//!
//! Generates little-endian uncompressed 16-bit DNG files compliant with TIFF/EP
//! and DNG 1.4 specifications.

#![allow(clippy::unnecessary_cast, clippy::manual_is_multiple_of)]

use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct DngBuilder {
    pub width: u32,
    pub height: u32,
    pub bits_per_sample: u16,
    pub photometric: u16,
    pub samples_per_pixel: u16,
    pub black_level: u32,
    pub white_level: u32,
    pub cfa_repeat_dim: [u16; 2],
    pub cfa_pattern: Vec<u8>,
    pub color_matrix1: Option<[f32; 9]>,
    pub calibration_illuminant1: u16,
    pub as_shot_neutral: Option<[f32; 3]>,
    pub make: String,
    pub model: String,
    pub unique_camera_model: String,
    pub orientation: Option<u16>,
    pub active_area: Option<[u32; 4]>,
    pub default_crop_origin: Option<[u32; 2]>,
    pub default_crop_size: Option<[u32; 2]>,
    pub preview_ifd0: Option<PreviewIfd0>,
}

#[derive(Debug, Clone)]
pub struct PreviewIfd0 {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>, // 8-bit RGB (width * height * 3)
}

impl DngBuilder {
    pub fn new_bayer(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            bits_per_sample: 16,
            photometric: 32803, // CFA
            samples_per_pixel: 1,
            black_level: 512,
            white_level: 16383,
            cfa_repeat_dim: [2, 2],
            cfa_pattern: vec![0, 1, 1, 2], // RGGB (0=R, 1=G, 2=B)
            color_matrix1: Some(TEST_CAMERA_MATRIX),
            calibration_illuminant1: 21, // D65
            as_shot_neutral: Some([1.0, 1.0, 1.0]),
            make: "Rollchrome".to_string(),
            model: "Synthetic Bayer".to_string(),
            unique_camera_model: "Rollchrome Synthetic Bayer".to_string(),
            orientation: None,
            active_area: None,
            default_crop_origin: None,
            default_crop_size: None,
            preview_ifd0: None,
        }
    }

    pub fn new_xtrans(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            bits_per_sample: 16,
            photometric: 32803, // CFA
            samples_per_pixel: 1,
            black_level: 512,
            white_level: 16383,
            cfa_repeat_dim: [6, 6],
            cfa_pattern: XTRANS_PATTERN.to_vec(),
            color_matrix1: Some(TEST_CAMERA_MATRIX),
            calibration_illuminant1: 21, // D65
            as_shot_neutral: Some([1.0, 1.0, 1.0]),
            make: "Rollchrome".to_string(),
            model: "Synthetic X-Trans".to_string(),
            unique_camera_model: "Rollchrome Synthetic X-Trans".to_string(),
            orientation: None,
            active_area: None,
            default_crop_origin: None,
            default_crop_size: None,
            preview_ifd0: None,
        }
    }

    pub fn build(&self, raw_pixels: &[u16]) -> Vec<u8> {
        let mut buffer = Vec::new();
        // TIFF Header: little-endian
        buffer.extend_from_slice(b"II");
        buffer.extend_from_slice(&42u16.to_le_bytes());

        if let Some(preview) = &self.preview_ifd0 {
            // Test 24 layout:
            // IFD0 is 8-bit RGB preview, with a SubIFDs pointer to the Raw IFD.
            // Offset to IFD0 is 8.
            let ifd0_offset = 8u32;
            buffer.extend_from_slice(&ifd0_offset.to_le_bytes());

            // Build Raw IFD tags (SubIFD)
            let mut raw_tags = BTreeMap::new();
            raw_tags.insert(254, TiffEntry::long(0)); // NewSubFileType = 0 (primary raw)
            raw_tags.insert(256, TiffEntry::long(self.width));
            raw_tags.insert(257, TiffEntry::long(self.height));
            raw_tags.insert(258, TiffEntry::short(self.bits_per_sample));
            raw_tags.insert(259, TiffEntry::short(1)); // Uncompressed
            raw_tags.insert(262, TiffEntry::short(self.photometric));
            raw_tags.insert(277, TiffEntry::short(self.samples_per_pixel));
            raw_tags.insert(278, TiffEntry::long(self.height));
            raw_tags.insert(284, TiffEntry::short(1)); // PlanarConfiguration = 1
            raw_tags.insert(
                279,
                TiffEntry::long(
                    (self.width * self.height * self.samples_per_pixel as u32 * 2) as u32,
                ),
            );
            if self.photometric == 32803 {
                raw_tags.insert(33421, TiffEntry::shorts(&self.cfa_repeat_dim));
                raw_tags.insert(33422, TiffEntry::bytes(&self.cfa_pattern));
            }
            raw_tags.insert(50714, TiffEntry::long(self.black_level));
            raw_tags.insert(50717, TiffEntry::long(self.white_level));

            // Build IFD0 tags
            let mut ifd0_tags = BTreeMap::new();
            ifd0_tags.insert(254, TiffEntry::long(1)); // NewSubFileType = 1 (reduced resolution / preview)
            ifd0_tags.insert(256, TiffEntry::long(preview.width));
            ifd0_tags.insert(257, TiffEntry::long(preview.height));
            ifd0_tags.insert(258, TiffEntry::shorts(&[8, 8, 8])); // 8-bit RGB
            ifd0_tags.insert(259, TiffEntry::short(1)); // Uncompressed
            ifd0_tags.insert(262, TiffEntry::short(2)); // RGB
            ifd0_tags.insert(271, TiffEntry::ascii(&self.make));
            ifd0_tags.insert(272, TiffEntry::ascii(&self.model));
            ifd0_tags.insert(277, TiffEntry::short(3));
            ifd0_tags.insert(278, TiffEntry::long(preview.height));
            ifd0_tags.insert(
                279,
                TiffEntry::long((preview.width * preview.height * 3) as u32),
            );
            ifd0_tags.insert(284, TiffEntry::short(1));
            ifd0_tags.insert(50706, TiffEntry::bytes(&[1, 4, 0, 0])); // DNGVersion
            ifd0_tags.insert(50708, TiffEntry::ascii(&self.unique_camera_model));
            if let Some(mat) = self.color_matrix1 {
                ifd0_tags.insert(50721, TiffEntry::srationals_from_f32(&mat));
            }
            ifd0_tags.insert(50778, TiffEntry::short(self.calibration_illuminant1));
            if let Some(neutral) = self.as_shot_neutral {
                ifd0_tags.insert(50728, TiffEntry::rationals_from_f32(&neutral));
            }

            // Serialization with two IFDs and pixel buffers
            serialize_two_ifds(
                &mut buffer,
                ifd0_tags,
                &preview.pixels,
                raw_tags,
                raw_pixels,
            );
        } else {
            // Standard single IFD DNG
            let ifd_offset = 8u32;
            buffer.extend_from_slice(&ifd_offset.to_le_bytes());

            let mut tags = BTreeMap::new();
            tags.insert(254, TiffEntry::long(0)); // NewSubFileType = 0
            tags.insert(256, TiffEntry::long(self.width));
            tags.insert(257, TiffEntry::long(self.height));
            if self.samples_per_pixel == 3 {
                tags.insert(
                    258,
                    TiffEntry::shorts(&[
                        self.bits_per_sample,
                        self.bits_per_sample,
                        self.bits_per_sample,
                    ]),
                );
            } else {
                tags.insert(258, TiffEntry::short(self.bits_per_sample));
            }
            tags.insert(259, TiffEntry::short(1)); // Uncompressed
            tags.insert(262, TiffEntry::short(self.photometric));
            tags.insert(271, TiffEntry::ascii(&self.make));
            tags.insert(272, TiffEntry::ascii(&self.model));
            if let Some(ori) = self.orientation {
                tags.insert(274, TiffEntry::short(ori));
            }
            tags.insert(277, TiffEntry::short(self.samples_per_pixel));
            tags.insert(278, TiffEntry::long(self.height));
            tags.insert(
                279,
                TiffEntry::long(
                    (self.width * self.height * self.samples_per_pixel as u32 * 2) as u32,
                ),
            );
            tags.insert(284, TiffEntry::short(1));
            if self.photometric == 32803 {
                tags.insert(33421, TiffEntry::shorts(&self.cfa_repeat_dim));
                tags.insert(33422, TiffEntry::bytes(&self.cfa_pattern));
            }
            tags.insert(50706, TiffEntry::bytes(&[1, 4, 0, 0])); // DNGVersion
            tags.insert(50708, TiffEntry::ascii(&self.unique_camera_model));
            if let Some(aa) = self.active_area {
                tags.insert(50829, TiffEntry::longs(&aa)); // ActiveArea (tag 50829 / 0xC68D)
            }
            if self.samples_per_pixel == 3 {
                tags.insert(
                    50714,
                    TiffEntry::longs(&[self.black_level, self.black_level, self.black_level]),
                );
                tags.insert(
                    50717,
                    TiffEntry::longs(&[self.white_level, self.white_level, self.white_level]),
                );
            } else {
                tags.insert(50714, TiffEntry::long(self.black_level));
                tags.insert(50717, TiffEntry::long(self.white_level));
            }
            if let Some(dco) = self.default_crop_origin {
                tags.insert(50719, TiffEntry::longs(&dco));
            }
            if let Some(dcs) = self.default_crop_size {
                tags.insert(50720, TiffEntry::longs(&dcs));
            }
            if let Some(mat) = self.color_matrix1 {
                tags.insert(50721, TiffEntry::srationals_from_f32(&mat));
            }
            if let Some(neutral) = self.as_shot_neutral {
                tags.insert(50728, TiffEntry::rationals_from_f32(&neutral));
            }
            tags.insert(50778, TiffEntry::short(self.calibration_illuminant1));

            serialize_single_ifd(&mut buffer, tags, raw_pixels);
        }

        buffer
    }
}

pub const TEST_CAMERA_MATRIX: [f32; 9] = [0.9, -0.3, -0.1, -0.4, 1.2, 0.2, -0.05, 0.15, 0.7];

pub const XTRANS_PATTERN: [u8; 36] = [
    1, 1, 0, 1, 1, 2, // G G R G G B
    1, 1, 2, 1, 1, 0, // G G B G G R
    2, 0, 1, 0, 2, 1, // B R G R B G
    1, 1, 2, 1, 1, 0, // G G B G G R
    1, 1, 0, 1, 1, 2, // G G R G G B
    0, 2, 1, 2, 0, 1, // R B G B R G
];

#[derive(Debug, Clone)]
struct TiffEntry {
    type_id: u16,
    count: u32,
    data: Vec<u8>,
}

impl TiffEntry {
    fn bytes(val: &[u8]) -> Self {
        Self {
            type_id: 1,
            count: val.len() as u32,
            data: val.to_vec(),
        }
    }

    fn ascii(s: &str) -> Self {
        let mut d = s.as_bytes().to_vec();
        d.push(0);
        Self {
            type_id: 2,
            count: d.len() as u32,
            data: d,
        }
    }

    fn short(val: u16) -> Self {
        Self {
            type_id: 3,
            count: 1,
            data: val.to_le_bytes().to_vec(),
        }
    }

    fn shorts(val: &[u16]) -> Self {
        let mut data = Vec::with_capacity(val.len() * 2);
        for &v in val {
            data.extend_from_slice(&v.to_le_bytes());
        }
        Self {
            type_id: 3,
            count: val.len() as u32,
            data,
        }
    }

    fn long(val: u32) -> Self {
        Self {
            type_id: 4,
            count: 1,
            data: val.to_le_bytes().to_vec(),
        }
    }

    fn longs(val: &[u32]) -> Self {
        let mut data = Vec::with_capacity(val.len() * 4);
        for &v in val {
            data.extend_from_slice(&v.to_le_bytes());
        }
        Self {
            type_id: 4,
            count: val.len() as u32,
            data,
        }
    }

    fn rationals_from_f32(val: &[f32]) -> Self {
        let mut data = Vec::with_capacity(val.len() * 8);
        for &v in val {
            let den = 10000u32;
            let num = (v * den as f32).round().max(0.0) as u32;
            data.extend_from_slice(&num.to_le_bytes());
            data.extend_from_slice(&den.to_le_bytes());
        }
        Self {
            type_id: 5,
            count: val.len() as u32,
            data,
        }
    }

    fn srationals_from_f32(val: &[f32]) -> Self {
        let mut data = Vec::with_capacity(val.len() * 8);
        for &v in val {
            let den = 10000i32;
            let num = (v * den as f32).round() as i32;
            data.extend_from_slice(&num.to_le_bytes());
            data.extend_from_slice(&den.to_le_bytes());
        }
        Self {
            type_id: 10,
            count: val.len() as u32,
            data,
        }
    }
}

fn serialize_single_ifd(
    buffer: &mut Vec<u8>,
    mut tags: BTreeMap<u16, TiffEntry>,
    raw_pixels: &[u16],
) {
    // StripOffsets (tag 273) will be calculated after IFD layout.
    // Placeholder for tag 273:
    tags.insert(273, TiffEntry::long(0));

    let num_entries = tags.len();
    let ifd_size = 2 + num_entries * 12 + 4;
    let ifd_offset = buffer.len();

    // Data payload starts right after IFD table
    let mut payload_offset = ifd_offset + ifd_size;

    // First pass: assign offsets for out-of-line tag data
    let mut resolved_entries = Vec::new();
    let mut payload = Vec::new();

    for (tag, entry) in tags {
        if tag == 273 {
            // Will set offset after payload
            resolved_entries.push((tag, entry.type_id, entry.count, 0u32, None));
            continue;
        }

        if entry.data.len() <= 4 {
            let mut val_bytes = [0u8; 4];
            val_bytes[..entry.data.len()].copy_from_slice(&entry.data);
            let val = u32::from_le_bytes(val_bytes);
            resolved_entries.push((tag, entry.type_id, entry.count, val, None));
        } else {
            let offset = payload_offset as u32;
            payload_offset += entry.data.len();
            resolved_entries.push((tag, entry.type_id, entry.count, offset, Some(entry.data)));
        }
    }

    // Now raw pixels sit after payload
    // Align pixels to 2-byte boundary
    if payload_offset % 2 != 0 {
        payload_offset += 1;
    }
    let pixel_strip_offset = payload_offset as u32;

    // Write IFD table
    buffer.extend_from_slice(&(num_entries as u16).to_le_bytes());
    for (tag, type_id, count, mut val_or_offset, data_opt) in resolved_entries {
        if tag == 273 {
            val_or_offset = pixel_strip_offset;
        }
        buffer.extend_from_slice(&tag.to_le_bytes());
        buffer.extend_from_slice(&type_id.to_le_bytes());
        buffer.extend_from_slice(&count.to_le_bytes());
        buffer.extend_from_slice(&val_or_offset.to_le_bytes());
        if let Some(d) = data_opt {
            payload.extend_from_slice(&d);
        }
    }
    // Next IFD = 0
    buffer.extend_from_slice(&0u32.to_le_bytes());

    // Write tag payload
    buffer.extend_from_slice(&payload);
    if buffer.len() % 2 != 0 {
        buffer.push(0);
    }

    // Write pixel strip
    for &p in raw_pixels {
        buffer.extend_from_slice(&p.to_le_bytes());
    }
}

fn serialize_two_ifds(
    buffer: &mut Vec<u8>,
    mut ifd0_tags: BTreeMap<u16, TiffEntry>,
    preview_pixels: &[u8],
    mut raw_tags: BTreeMap<u16, TiffEntry>,
    raw_pixels: &[u16],
) {
    // Placeholders for strip offsets and subifds
    ifd0_tags.insert(273, TiffEntry::long(0)); // Preview StripOffsets
    ifd0_tags.insert(330, TiffEntry::long(0)); // SubIFDs
    raw_tags.insert(273, TiffEntry::long(0)); // Raw StripOffsets

    let ifd0_offset = buffer.len();
    let ifd0_size = 2 + ifd0_tags.len() * 12 + 4;
    let mut payload_offset = ifd0_offset + ifd0_size;

    // IFD0 entries
    let mut ifd0_resolved = Vec::new();
    let mut ifd0_payload = Vec::new();

    for (tag, entry) in ifd0_tags {
        if tag == 273 || tag == 330 {
            ifd0_resolved.push((tag, entry.type_id, entry.count, 0u32, None));
            continue;
        }
        if entry.data.len() <= 4 {
            let mut val_bytes = [0u8; 4];
            val_bytes[..entry.data.len()].copy_from_slice(&entry.data);
            let val = u32::from_le_bytes(val_bytes);
            ifd0_resolved.push((tag, entry.type_id, entry.count, val, None));
        } else {
            let offset = payload_offset as u32;
            payload_offset += entry.data.len();
            ifd0_resolved.push((tag, entry.type_id, entry.count, offset, Some(entry.data)));
        }
    }

    // Align to 2-byte boundary
    if payload_offset % 2 != 0 {
        payload_offset += 1;
    }
    let preview_strip_offset = payload_offset as u32;
    payload_offset += preview_pixels.len();

    // Now SubIFD (Raw IFD)
    if payload_offset % 2 != 0 {
        payload_offset += 1;
    }
    let sub_ifd_offset = payload_offset as u32;
    let raw_ifd_size = 2 + raw_tags.len() * 12 + 4;
    payload_offset += raw_ifd_size;

    let mut raw_resolved = Vec::new();
    let mut raw_payload = Vec::new();

    for (tag, entry) in raw_tags {
        if tag == 273 {
            raw_resolved.push((tag, entry.type_id, entry.count, 0u32, None));
            continue;
        }
        if entry.data.len() <= 4 {
            let mut val_bytes = [0u8; 4];
            val_bytes[..entry.data.len()].copy_from_slice(&entry.data);
            let val = u32::from_le_bytes(val_bytes);
            raw_resolved.push((tag, entry.type_id, entry.count, val, None));
        } else {
            let offset = payload_offset as u32;
            payload_offset += entry.data.len();
            raw_resolved.push((tag, entry.type_id, entry.count, offset, Some(entry.data)));
        }
    }

    if payload_offset % 2 != 0 {
        payload_offset += 1;
    }
    let raw_strip_offset = payload_offset as u32;

    // Write IFD0 table
    buffer.extend_from_slice(&(ifd0_resolved.len() as u16).to_le_bytes());
    for (tag, type_id, count, mut val_or_offset, data_opt) in ifd0_resolved {
        if tag == 273 {
            val_or_offset = preview_strip_offset;
        } else if tag == 330 {
            val_or_offset = sub_ifd_offset;
        }
        buffer.extend_from_slice(&tag.to_le_bytes());
        buffer.extend_from_slice(&type_id.to_le_bytes());
        buffer.extend_from_slice(&count.to_le_bytes());
        buffer.extend_from_slice(&val_or_offset.to_le_bytes());
        if let Some(d) = data_opt {
            ifd0_payload.extend_from_slice(&d);
        }
    }
    buffer.extend_from_slice(&0u32.to_le_bytes());
    buffer.extend_from_slice(&ifd0_payload);

    if buffer.len() % 2 != 0 {
        buffer.push(0);
    }
    // Write preview pixels
    buffer.extend_from_slice(preview_pixels);

    if buffer.len() % 2 != 0 {
        buffer.push(0);
    }
    debug_assert_eq!(buffer.len(), sub_ifd_offset as usize);

    // Write SubIFD table
    buffer.extend_from_slice(&(raw_resolved.len() as u16).to_le_bytes());
    for (tag, type_id, count, mut val_or_offset, data_opt) in raw_resolved {
        if tag == 273 {
            val_or_offset = raw_strip_offset;
        }
        buffer.extend_from_slice(&tag.to_le_bytes());
        buffer.extend_from_slice(&type_id.to_le_bytes());
        buffer.extend_from_slice(&count.to_le_bytes());
        buffer.extend_from_slice(&val_or_offset.to_le_bytes());
        if let Some(d) = data_opt {
            raw_payload.extend_from_slice(&d);
        }
    }
    buffer.extend_from_slice(&0u32.to_le_bytes());
    buffer.extend_from_slice(&raw_payload);

    if buffer.len() % 2 != 0 {
        buffer.push(0);
    }
    debug_assert_eq!(buffer.len(), raw_strip_offset as usize);

    // Write raw pixels
    for &p in raw_pixels {
        buffer.extend_from_slice(&p.to_le_bytes());
    }
}
