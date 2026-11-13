//! Developed RAW rendering and encoding to JPEG and TIFF (DV-2a).
//!
//! Converts developed ProPhoto images into deliverable JPEG (sRGB) or archival
//! 16-bit TIFF (ProPhoto RGB with embedded ICC profile), with optional 3D LUT look.

use image::DynamicImage;
use phototools_core::error::Error;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::media::develop::colour;
use crate::media::develop::tone;
use crate::media::develop::Developed;
use crate::media::edit::lut::Lut;

/// Format for rendered output files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OutputFormat {
    /// 8-bit sRGB JPEG with quality in 1..=100.
    Jpeg { quality: u8 },
    /// 16-bit ProPhoto RGB TIFF with embedded ICC profile and Deflate compression.
    Tiff,
}

/// Space in which a Look is applied (DV-3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LookSpace {
    /// sRGB space (library .cube LUTs).
    Srgb,
    /// 16-bit ROMM-encoded ProPhoto RGB space (built-in looks).
    ProPhoto,
}

/// A 3D LUT look together with its expected colour space (DV-3).
#[derive(Debug, Clone, PartialEq)]
pub struct Look {
    pub lut: Lut,
    pub space: LookSpace,
}

impl Look {
    /// Creates a look operating in sRGB space.
    pub fn srgb(lut: Lut) -> Self {
        Self {
            lut,
            space: LookSpace::Srgb,
        }
    }

    /// Creates a look operating in ProPhoto RGB space.
    pub fn prophoto(lut: Lut) -> Self {
        Self {
            lut,
            space: LookSpace::ProPhoto,
        }
    }
}

/// Result of rendering a developed RAW image.
#[derive(Debug)]
pub struct Rendered {
    /// Encoded image file bytes.
    pub bytes: Vec<u8>,
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// File extension: "jpg" or "tif".
    pub extension: &'static str,
}

/// Convert sRGB code in [0.0, 1.0] to linear light.
#[inline]
pub(crate) fn srgb_to_linear(s: f32) -> f32 {
    let s = s.clamp(0.0, 1.0);
    if s <= 0.04045 {
        s / 12.92
    } else {
        ((s + 0.055) / 1.055).powf(2.4)
    }
}

/// Convert linear light [0.0, 1.0] to sRGB code.
#[inline]
pub(crate) fn linear_to_srgb(l: f32) -> f32 {
    let l = l.clamp(0.0, 1.0);
    if l <= 0.0031308 {
        l * 12.92
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    }
}

/// Builds the ICC profile for 16-bit ProPhoto RGB matching DV-1's encoding.
///
/// Uses `moxcms` with ProPhoto primaries (D50) and replaces red, green, and blue
/// TRCs with ICC parametric curve type 3: g = 1.8, a = 1.0, b = 0.0, c = 1/16, d = 1/32.
pub fn prophoto_icc_profile() -> Result<Vec<u8>, Error> {
    let mut profile = moxcms::ColorProfile::new_pro_photo_rgb();
    let curve = moxcms::ToneReprCurve::Parametric(vec![1.8, 1.0, 0.0, 1.0 / 16.0, 1.0 / 32.0]);
    profile.red_trc = Some(curve.clone());
    profile.green_trc = Some(curve.clone());
    profile.blue_trc = Some(curve);
    profile.description = Some(moxcms::ProfileText::Localizable(vec![
        moxcms::LocalizableString::new(
            "en".to_string(),
            "US".to_string(),
            "ProPhoto RGB (ROMM)".to_string(),
        ),
    ]));
    profile
        .encode()
        .map_err(|e| Error::Internal(format!("Failed to encode ICC profile: {e:?}")))
}

/// Renders a developed image to sRGB 8-bit image buffer, optionally applying a Look.
///
/// Shared between JPEG rendering, `prophoto_to_srgb8`, and unit tests.
pub(crate) fn render_srgb8_image(developed: &Developed, look: Option<&Look>) -> image::RgbImage {
    let w = developed.image.width();
    let h = developed.image.height();
    let m = colour::prophoto_to_srgb_matrix();

    let raw_u16 = developed.image.as_raw();
    let u8_data: Vec<u8> = raw_u16
        .par_chunks_exact(3)
        .flat_map(|pixel| {
            match look {
                None => {
                    let r_lin = tone::prophoto_decode(pixel[0] as f32 / 65535.0);
                    let g_lin = tone::prophoto_decode(pixel[1] as f32 / 65535.0);
                    let b_lin = tone::prophoto_decode(pixel[2] as f32 / 65535.0);

                    let srgb_lin = colour::mul_3x3_vec(m, [r_lin, g_lin, b_lin]);
                    let s = [
                        linear_to_srgb(srgb_lin[0]),
                        linear_to_srgb(srgb_lin[1]),
                        linear_to_srgb(srgb_lin[2]),
                    ];

                    [
                        (s[0] * 255.0).round().clamp(0.0, 255.0) as u8,
                        (s[1] * 255.0).round().clamp(0.0, 255.0) as u8,
                        (s[2] * 255.0).round().clamp(0.0, 255.0) as u8,
                    ]
                }
                Some(look) if look.space == LookSpace::Srgb => {
                    let r_lin = tone::prophoto_decode(pixel[0] as f32 / 65535.0);
                    let g_lin = tone::prophoto_decode(pixel[1] as f32 / 65535.0);
                    let b_lin = tone::prophoto_decode(pixel[2] as f32 / 65535.0);

                    let srgb_lin = colour::mul_3x3_vec(m, [r_lin, g_lin, b_lin]);
                    let mut s = [
                        linear_to_srgb(srgb_lin[0]),
                        linear_to_srgb(srgb_lin[1]),
                        linear_to_srgb(srgb_lin[2]),
                    ];

                    let sampled = look.lut.sample(s);
                    s[0] = sampled[0].clamp(0.0, 1.0);
                    s[1] = sampled[1].clamp(0.0, 1.0);
                    s[2] = sampled[2].clamp(0.0, 1.0);

                    [
                        (s[0] * 255.0).round().clamp(0.0, 255.0) as u8,
                        (s[1] * 255.0).round().clamp(0.0, 255.0) as u8,
                        (s[2] * 255.0).round().clamp(0.0, 255.0) as u8,
                    ]
                }
                Some(look) => {
                    // ProPhoto look: sample in ROMM ProPhoto space, then convert looked ProPhoto to sRGB
                    let in_pro = [
                        pixel[0] as f32 / 65535.0,
                        pixel[1] as f32 / 65535.0,
                        pixel[2] as f32 / 65535.0,
                    ];
                    let sampled = look.lut.sample(in_pro);
                    let pro_enc = [
                        sampled[0].clamp(0.0, 1.0),
                        sampled[1].clamp(0.0, 1.0),
                        sampled[2].clamp(0.0, 1.0),
                    ];

                    let r_lin = tone::prophoto_decode(pro_enc[0]);
                    let g_lin = tone::prophoto_decode(pro_enc[1]);
                    let b_lin = tone::prophoto_decode(pro_enc[2]);

                    let srgb_lin = colour::mul_3x3_vec(m, [r_lin, g_lin, b_lin]);
                    let s = [
                        linear_to_srgb(srgb_lin[0]),
                        linear_to_srgb(srgb_lin[1]),
                        linear_to_srgb(srgb_lin[2]),
                    ];

                    [
                        (s[0] * 255.0).round().clamp(0.0, 255.0) as u8,
                        (s[1] * 255.0).round().clamp(0.0, 255.0) as u8,
                        (s[2] * 255.0).round().clamp(0.0, 255.0) as u8,
                    ]
                }
            }
        })
        .collect();

    image::RgbImage::from_raw(w, h, u8_data).unwrap_or_else(|| image::RgbImage::new(w, h))
}

/// Renders 16-bit ProPhoto pixels with a Look applied.
pub fn render_tiff_u16(developed: &Developed, look: &Look) -> Vec<u16> {
    let raw_u16 = developed.image.as_raw();

    match look.space {
        LookSpace::Srgb => {
            let m_pro_to_srgb = colour::prophoto_to_srgb_matrix();
            let m_srgb_to_pro = colour::srgb_to_prophoto_matrix();

            raw_u16
                .par_chunks_exact(3)
                .flat_map(|pixel| {
                    let r_lin = tone::prophoto_decode(pixel[0] as f32 / 65535.0);
                    let g_lin = tone::prophoto_decode(pixel[1] as f32 / 65535.0);
                    let b_lin = tone::prophoto_decode(pixel[2] as f32 / 65535.0);

                    let srgb_lin = colour::mul_3x3_vec(m_pro_to_srgb, [r_lin, g_lin, b_lin]);
                    let s_in = [
                        linear_to_srgb(srgb_lin[0]),
                        linear_to_srgb(srgb_lin[1]),
                        linear_to_srgb(srgb_lin[2]),
                    ];

                    let sampled = look.lut.sample(s_in);
                    let s_out = [
                        sampled[0].clamp(0.0, 1.0),
                        sampled[1].clamp(0.0, 1.0),
                        sampled[2].clamp(0.0, 1.0),
                    ];

                    let lin_srgb = [
                        srgb_to_linear(s_out[0]),
                        srgb_to_linear(s_out[1]),
                        srgb_to_linear(s_out[2]),
                    ];

                    let lin_pro = colour::mul_3x3_vec(m_srgb_to_pro, lin_srgb);
                    let mut out = [0u16; 3];
                    for c in 0..3 {
                        let enc = tone::prophoto_encode(lin_pro[c].max(0.0));
                        out[c] = (enc * 65535.0).round().clamp(0.0, 65535.0) as u16;
                    }
                    out
                })
                .collect()
        }
        LookSpace::ProPhoto => raw_u16
            .par_chunks_exact(3)
            .flat_map(|pixel| {
                let in_pro = [
                    pixel[0] as f32 / 65535.0,
                    pixel[1] as f32 / 65535.0,
                    pixel[2] as f32 / 65535.0,
                ];
                let sampled = look.lut.sample(in_pro);
                [
                    (sampled[0].clamp(0.0, 1.0) * 65535.0)
                        .round()
                        .clamp(0.0, 65535.0) as u16,
                    (sampled[1].clamp(0.0, 1.0) * 65535.0)
                        .round()
                        .clamp(0.0, 65535.0) as u16,
                    (sampled[2].clamp(0.0, 1.0) * 65535.0)
                        .round()
                        .clamp(0.0, 65535.0) as u16,
                ]
            })
            .collect(),
    }
}

pub fn encode_tiff(
    width: u32,
    height: u32,
    pixels: &[u16],
    icc_profile: &[u8],
) -> Result<Vec<u8>, Error> {
    use std::io::Cursor;
    use tiff::encoder::{colortype::RGB16, Compression, DeflateLevel, Predictor, TiffEncoder};
    use tiff::tags::Tag;

    let mut buffer = Cursor::new(Vec::new());
    let mut encoder = TiffEncoder::new(&mut buffer)
        .map_err(|e| Error::Internal(format!("TIFF encoder init failed: {e}")))?
        .with_compression(Compression::Deflate(DeflateLevel::default()))
        .with_predictor(Predictor::Horizontal);

    let mut image_encoder = encoder
        .new_image::<RGB16>(width, height)
        .map_err(|e| Error::Internal(format!("TIFF image init failed: {e}")))?;

    image_encoder
        .encoder()
        .write_tag(Tag::IccProfile, icc_profile)
        .map_err(|e| Error::Internal(format!("Failed to write ICC profile tag: {e}")))?;

    image_encoder
        .write_data(pixels)
        .map_err(|e| Error::Internal(format!("Failed to write TIFF pixel data: {e}")))?;

    Ok(buffer.into_inner())
}

/// Renders a developed RAW image to the specified format with an optional film look.
pub fn render(
    developed: &Developed,
    look: Option<&Look>,
    format: OutputFormat,
) -> Result<Rendered, Error> {
    let width = developed.image.width();
    let height = developed.image.height();

    match format {
        OutputFormat::Jpeg { quality } => {
            if !(1..=100).contains(&quality) {
                return Err(Error::Refused(format!(
                    "JPEG quality {quality} is outside 1..=100"
                )));
            }
            let srgb_img = render_srgb8_image(developed, look);
            let dyn_img = DynamicImage::ImageRgb8(srgb_img);
            let opts = phototools_core::media::jpeg::JpegOptions::deliverable(quality);
            let bytes = phototools_core::media::jpeg::encode(&dyn_img, &opts)?;
            Ok(Rendered {
                bytes,
                width,
                height,
                extension: "jpg",
            })
        }
        OutputFormat::Tiff => {
            let icc_profile = prophoto_icc_profile()?;
            let bytes = match look {
                None => encode_tiff(width, height, developed.image.as_raw(), &icc_profile)?,
                Some(look) => {
                    let pixels = render_tiff_u16(developed, look);
                    encode_tiff(width, height, &pixels, &icc_profile)?
                }
            };
            Ok(Rendered {
                bytes,
                width,
                height,
                extension: "tif",
            })
        }
    }
}

/// Size of a frame downscaled so its long edge is at most `max_edge`. **Downscale only**: a frame
/// already within the limit keeps its size. Same rule as the preview and as PhotoTools' F6 (floor,
/// at least 1), so a long edge means the same thing everywhere.
pub fn downscaled_size(width: u32, height: u32, max_edge: u32) -> (u32, u32) {
    let long = width.max(height);
    if max_edge == 0 || long <= max_edge {
        return (width, height);
    }
    let ratio = max_edge as f64 / long as f64;
    (
        ((width as f64 * ratio).floor() as u32).max(1),
        ((height as f64 * ratio).floor() as u32).max(1),
    )
}

/// Downscales interleaved 16-bit RGB so the long edge is at most `max_edge` (DV-7).
pub(crate) fn downscale_u16_rgb(
    width: u32,
    height: u32,
    pixels: &[u16],
    max_edge: u32,
) -> Result<(u32, u32, Vec<u16>), Error> {
    use fast_image_resize as fr;

    let (new_w, new_h) = downscaled_size(width, height, max_edge);
    if (new_w, new_h) == (width, height) {
        return Ok((width, height, pixels.to_vec()));
    }
    let bytes: Vec<u8> = pixels.iter().flat_map(|v| v.to_ne_bytes()).collect();
    let src = fr::images::ImageRef::new(width, height, &bytes, fr::PixelType::U16x3)
        .map_err(|e| Error::Internal(format!("resize 16-bit source: {e:?}")))?;
    let mut dst = fr::images::Image::new(new_w, new_h, fr::PixelType::U16x3);
    fr::Resizer::new()
        .resize(&src, &mut dst, None)
        .map_err(|e| Error::Internal(format!("resize 16-bit: {e:?}")))?;
    let out = dst
        .into_vec()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u16::from_ne_bytes(*b))
        .collect();
    Ok((new_w, new_h, out))
}

/// [`render`], with the output downscaled to a long edge of at most `long_edge` pixels (DV-7).
///
/// The look is applied at full resolution and the result is downscaled, so a look's output is the
/// same picture at any size. With `None` this is exactly [`render`], byte for byte.
pub fn render_sized(
    developed: &Developed,
    look: Option<&Look>,
    format: OutputFormat,
    long_edge: Option<u32>,
) -> Result<Rendered, Error> {
    let Some(max_edge) = long_edge else {
        return render(developed, look, format);
    };
    let width = developed.image.width();
    let height = developed.image.height();

    match format {
        OutputFormat::Jpeg { quality } => {
            if !(1..=100).contains(&quality) {
                return Err(Error::Refused(format!(
                    "JPEG quality {quality} is outside 1..=100"
                )));
            }
            let full = DynamicImage::ImageRgb8(render_srgb8_image(developed, look));
            let sized = phototools_core::media::image_ops::downscale_to_max_edge(&full, max_edge)?;
            let opts = phototools_core::media::jpeg::JpegOptions::deliverable(quality);
            let bytes = phototools_core::media::jpeg::encode(&sized, &opts)?;
            Ok(Rendered {
                bytes,
                width: sized.width(),
                height: sized.height(),
                extension: "jpg",
            })
        }
        OutputFormat::Tiff => {
            let icc_profile = prophoto_icc_profile()?;
            let pixels = match look {
                None => developed.image.as_raw().clone(),
                Some(look) => render_tiff_u16(developed, look),
            };
            let (w, h, sized) = downscale_u16_rgb(width, height, &pixels, max_edge)?;
            let bytes = encode_tiff(w, h, &sized, &icc_profile)?;
            Ok(Rendered {
                bytes,
                width: w,
                height: h,
                extension: "tif",
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::develop::Demosaic;
    use image::ImageBuffer;

    fn make_developed(w: u32, h: u32, u16_pixels: Vec<u16>) -> Developed {
        let image = ImageBuffer::from_raw(w, h, u16_pixels).expect("valid image");
        Developed {
            image,
            orientation: 1,
            demosaic: Demosaic::XTrans,
        }
    }

    fn generate_identity_cube(size: usize) -> Vec<u8> {
        let mut s = format!("TITLE \"Identity\"\nLUT_3D_SIZE {}\n", size);
        for b in 0..size {
            for g in 0..size {
                for r in 0..size {
                    let rf = r as f32 / (size - 1) as f32;
                    let gf = g as f32 / (size - 1) as f32;
                    let bf = b as f32 / (size - 1) as f32;
                    use std::fmt::Write;
                    let _ = writeln!(&mut s, "{rf:.6} {gf:.6} {bf:.6}");
                }
            }
        }
        s.into_bytes()
    }

    fn generate_inverting_cube(size: usize) -> Vec<u8> {
        let mut s = format!("TITLE \"Invert\"\nLUT_3D_SIZE {}\n", size);
        for b in 0..size {
            for g in 0..size {
                for r in 0..size {
                    let rf = 1.0 - r as f32 / (size - 1) as f32;
                    let gf = 1.0 - g as f32 / (size - 1) as f32;
                    let bf = 1.0 - b as f32 / (size - 1) as f32;
                    use std::fmt::Write;
                    let _ = writeln!(&mut s, "{rf:.6} {gf:.6} {bf:.6}");
                }
            }
        }
        s.into_bytes()
    }

    // Helper to build a Developed image with 4 flat 32x32 patches inside sRGB
    fn four_patch_developed() -> Developed {
        let patch_size = 32u32;
        let w = patch_size * 2;
        let h = patch_size * 2;

        let m_srgb_to_pro = colour::srgb_to_prophoto_matrix();

        // Four in-gamut sRGB colors:
        // 0: Grey [128, 128, 128]
        // 1: Skin tone [200, 150, 120]
        // 2: Sky blue [100, 160, 230]
        // 3: Foliage green [70, 140, 60]
        let srgb_patches = [
            [128u8, 128, 128],
            [200, 150, 120],
            [100, 160, 230],
            [70, 140, 60],
        ];

        let mut pro_u16_patches = [[0u16; 3]; 4];
        for (i, srgb) in srgb_patches.iter().enumerate() {
            let s_lin = [
                srgb_to_linear(srgb[0] as f32 / 255.0),
                srgb_to_linear(srgb[1] as f32 / 255.0),
                srgb_to_linear(srgb[2] as f32 / 255.0),
            ];
            let pro_lin = colour::mul_3x3_vec(m_srgb_to_pro, s_lin);
            pro_u16_patches[i] = [
                (tone::prophoto_encode(pro_lin[0]) * 65535.0)
                    .round()
                    .clamp(0.0, 65535.0) as u16,
                (tone::prophoto_encode(pro_lin[1]) * 65535.0)
                    .round()
                    .clamp(0.0, 65535.0) as u16,
                (tone::prophoto_encode(pro_lin[2]) * 65535.0)
                    .round()
                    .clamp(0.0, 65535.0) as u16,
            ];
        }

        let mut raw = vec![0u16; (w * h * 3) as usize];
        for y in 0..h {
            for x in 0..w {
                let patch_idx = match (y < patch_size, x < patch_size) {
                    (true, true) => 0,
                    (true, false) => 1,
                    (false, true) => 2,
                    (false, false) => 3,
                };
                let pixel = pro_u16_patches[patch_idx];
                let offset = (y * w + x) as usize * 3;
                raw[offset] = pixel[0];
                raw[offset + 1] = pixel[1];
                raw[offset + 2] = pixel[2];
            }
        }

        make_developed(w, h, raw)
    }

    #[test]
    fn the_tiff_is_16_bit_prophoto_and_keeps_the_developed_pixels_without_a_look() {
        let w = 32;
        let h = 32;
        let mut raw = Vec::with_capacity((w * h * 3) as usize);
        for i in 0..(w * h * 3) {
            raw.push(((i * 73) % 65536) as u16);
        }
        let dev = make_developed(w, h, raw.clone());
        let rendered = render(&dev, None, OutputFormat::Tiff).unwrap();
        assert_eq!(rendered.extension, "tif");
        assert_eq!(rendered.width, w);
        assert_eq!(rendered.height, h);

        let mut decoder =
            tiff::decoder::Decoder::new(std::io::Cursor::new(&rendered.bytes)).unwrap();
        assert_eq!(decoder.dimensions().unwrap(), (w, h));
        assert_eq!(decoder.colortype().unwrap(), tiff::ColorType::RGB(16));
        match decoder.read_image().unwrap() {
            tiff::decoder::DecodingResult::U16(pixels) => {
                assert_eq!(pixels.len(), raw.len());
                assert_eq!(pixels.as_slice(), raw.as_slice());
            }
            _ => panic!("expected U16 decoding result"),
        }
    }

    #[test]
    fn the_tiff_carries_a_profile_that_decodes_as_its_own_encoding() {
        let w = 4;
        let h = 4;
        let raw = vec![30000u16; (w * h * 3) as usize];
        let dev = make_developed(w, h, raw);
        let rendered = render(&dev, None, OutputFormat::Tiff).unwrap();

        let mut decoder =
            tiff::decoder::Decoder::new(std::io::Cursor::new(&rendered.bytes)).unwrap();
        let tag_val = decoder
            .find_tag(tiff::tags::Tag::IccProfile)
            .unwrap()
            .expect("tag 34675");
        let icc_bytes: Vec<u8> = match tag_val {
            tiff::decoder::ifd::Value::List(l) => l
                .into_iter()
                .map(|v| match v {
                    tiff::decoder::ifd::Value::Byte(b) => b,
                    _ => panic!("expected Byte in list"),
                })
                .collect(),
            tiff::decoder::ifd::Value::Byte(b) => vec![b],
            _ => panic!("unexpected tag type"),
        };

        let profile = moxcms::ColorProfile::new_from_slice(&icc_bytes).unwrap();
        let expected_profile = moxcms::ColorProfile::new_pro_photo_rgb();

        assert!((profile.red_colorant.x - expected_profile.red_colorant.x).abs() <= 1e-3);
        assert!((profile.red_colorant.y - expected_profile.red_colorant.y).abs() <= 1e-3);
        assert!((profile.red_colorant.z - expected_profile.red_colorant.z).abs() <= 1e-3);
        assert!((profile.green_colorant.x - expected_profile.green_colorant.x).abs() <= 1e-3);
        assert!((profile.green_colorant.y - expected_profile.green_colorant.y).abs() <= 1e-3);
        assert!((profile.green_colorant.z - expected_profile.green_colorant.z).abs() <= 1e-3);
        assert!((profile.blue_colorant.x - expected_profile.blue_colorant.x).abs() <= 1e-3);
        assert!((profile.blue_colorant.y - expected_profile.blue_colorant.y).abs() <= 1e-3);
        assert!((profile.blue_colorant.z - expected_profile.blue_colorant.z).abs() <= 1e-3);

        let red_trc = profile.red_trc.as_ref().expect("red TRC present");
        let eval = red_trc.make_linear_evaluator().expect("evaluator created");
        let val_half = eval.evaluate_value(0.5);
        let expected_half = 0.5f32.powf(1.8);
        assert!(
            (val_half - expected_half).abs() <= 1e-4,
            "at 0.5 got {val_half}, expected {expected_half}"
        );

        let val_small = eval.evaluate_value(1.0 / 64.0);
        let expected_small = (1.0 / 64.0) / 16.0;
        assert!(
            (val_small - expected_small).abs() <= 1e-4,
            "at 1/64 got {val_small}, expected {expected_small}"
        );
    }

    #[test]
    fn the_jpeg_without_a_look_matches_the_srgb_conversion() {
        let dev = four_patch_developed();
        let expected = crate::media::develop::prophoto_to_srgb8(&dev);

        let rendered = render(&dev, None, OutputFormat::Jpeg { quality: 95 }).unwrap();
        assert_eq!(rendered.extension, "jpg");

        let decoded = image::load_from_memory(&rendered.bytes).unwrap().to_rgb8();

        let centres = [(16u32, 16u32), (48, 16), (16, 48), (48, 48)];
        for (cx, cy) in centres {
            let p_dec = decoded.get_pixel(cx, cy);
            let p_exp = expected.get_pixel(cx, cy);
            for c in 0..3 {
                let diff = (p_dec[c] as i32 - p_exp[c] as i32).abs();
                assert!(
                    diff <= 2,
                    "at ({cx}, {cy}) ch {c}: got {}, expected {}, diff {}",
                    p_dec[c],
                    p_exp[c],
                    diff
                );
            }
        }
    }

    #[test]
    fn an_identity_look_leaves_colours_inside_srgb_unchanged() {
        let dev = four_patch_developed();
        let cube_bytes = generate_identity_cube(17);
        let identity_lut = Lut::from_cube("identity.cube", &cube_bytes).unwrap();
        let identity_look = Look::srgb(identity_lut);

        // 1. TIFF: pixels within +/-64/65535 of no-look TIFF for the four patches
        let rendered_no_look = render(&dev, None, OutputFormat::Tiff).unwrap();
        let rendered_with_look = render(&dev, Some(&identity_look), OutputFormat::Tiff).unwrap();

        let mut d1 =
            tiff::decoder::Decoder::new(std::io::Cursor::new(&rendered_no_look.bytes)).unwrap();
        let p1 = match d1.read_image().unwrap() {
            tiff::decoder::DecodingResult::U16(v) => v,
            _ => panic!("expected U16"),
        };
        let mut d2 =
            tiff::decoder::Decoder::new(std::io::Cursor::new(&rendered_with_look.bytes)).unwrap();
        let p2 = match d2.read_image().unwrap() {
            tiff::decoder::DecodingResult::U16(v) => v,
            _ => panic!("expected U16"),
        };

        let centres = [(16u32, 16u32), (48, 16), (16, 48), (48, 48)];
        for (cx, cy) in centres {
            let idx = (cy * dev.image.width() + cx) as usize * 3;
            for c in 0..3 {
                let diff = (p1[idx + c] as i32 - p2[idx + c] as i32).abs();
                assert!(
                    diff <= 64,
                    "TIFF at ({cx}, {cy}) ch {c}: no-look {}, with-look {}, diff {diff} > 64",
                    p1[idx + c],
                    p2[idx + c]
                );
            }
        }

        // 2. JPEG buffer before encoding within +/-1
        let buf_no_look = render_srgb8_image(&dev, None);
        let buf_with_look = render_srgb8_image(&dev, Some(&identity_look));

        for (cx, cy) in centres {
            let pix1 = buf_no_look.get_pixel(cx, cy);
            let pix2 = buf_with_look.get_pixel(cx, cy);
            for c in 0..3 {
                let diff = (pix1[c] as i32 - pix2[c] as i32).abs();
                assert!(
                    diff <= 1,
                    "u8 buffer at ({cx}, {cy}) ch {c}: no-look {}, with-look {}, diff {diff} > 1",
                    pix1[c],
                    pix2[c]
                );
            }
        }
    }

    #[test]
    fn a_look_confines_the_tiff_to_srgb_colours() {
        // Saturated ProPhoto green outside sRGB: pure [0, 65535, 0]
        let w = 8;
        let h = 8;
        let raw = vec![0u16, 65535, 0]
            .into_iter()
            .cycle()
            .take((w * h * 3) as usize)
            .collect();
        let dev = make_developed(w, h, raw);

        let m_pro_to_srgb = colour::prophoto_to_srgb_matrix();

        // Without look: converts to a component below -0.05
        let rendered_no_look = render(&dev, None, OutputFormat::Tiff).unwrap();
        let mut d1 =
            tiff::decoder::Decoder::new(std::io::Cursor::new(&rendered_no_look.bytes)).unwrap();
        let p1 = match d1.read_image().unwrap() {
            tiff::decoder::DecodingResult::U16(v) => v,
            _ => panic!("expected U16"),
        };
        let lin_no_look = [
            tone::prophoto_decode(p1[0] as f32 / 65535.0),
            tone::prophoto_decode(p1[1] as f32 / 65535.0),
            tone::prophoto_decode(p1[2] as f32 / 65535.0),
        ];
        let srgb_no_look = colour::mul_3x3_vec(m_pro_to_srgb, lin_no_look);
        assert!(
            srgb_no_look[0] < -0.05,
            "Expected red component < -0.05 without look, got {}",
            srgb_no_look[0]
        );

        // With identity look: converted back to linear sRGB, every component within -1e-3..=1+1e-3
        let cube_bytes = generate_identity_cube(17);
        let identity_lut = Lut::from_cube("identity.cube", &cube_bytes).unwrap();
        let identity_look = Look::srgb(identity_lut);
        let rendered_with_look = render(&dev, Some(&identity_look), OutputFormat::Tiff).unwrap();
        let mut d2 =
            tiff::decoder::Decoder::new(std::io::Cursor::new(&rendered_with_look.bytes)).unwrap();
        let p2 = match d2.read_image().unwrap() {
            tiff::decoder::DecodingResult::U16(v) => v,
            _ => panic!("expected U16"),
        };
        let lin_with_look = [
            tone::prophoto_decode(p2[0] as f32 / 65535.0),
            tone::prophoto_decode(p2[1] as f32 / 65535.0),
            tone::prophoto_decode(p2[2] as f32 / 65535.0),
        ];
        let srgb_with_look = colour::mul_3x3_vec(m_pro_to_srgb, lin_with_look);
        for (c, &val) in srgb_with_look.iter().enumerate() {
            assert!(
                (-1e-3..=1.0 + 1e-3).contains(&val),
                "With look ch {c}: got {val}, expected in [-1e-3, 1+1e-3]",
            );
        }
    }

    #[test]
    fn a_look_is_applied() {
        let w = 16;
        let h = 16;
        let raw = vec![32768u16; (w * h * 3) as usize];
        let dev = make_developed(w, h, raw);

        let base_srgb = render_srgb8_image(&dev, None);
        let s_val = base_srgb.get_pixel(8, 8)[0];

        let invert_lut = Lut::from_cube("invert.cube", &generate_inverting_cube(17)).unwrap();
        let invert_look = Look::srgb(invert_lut);
        let rendered =
            render(&dev, Some(&invert_look), OutputFormat::Jpeg { quality: 95 }).unwrap();

        let decoded = image::load_from_memory(&rendered.bytes).unwrap().to_rgb8();
        let dec_val = decoded.get_pixel(8, 8)[0];
        let expected = 255 - s_val;
        let diff = (dec_val as i32 - expected as i32).abs();
        assert!(
            diff <= 2,
            "dec_val {dec_val}, expected {expected}, diff {diff} > 2"
        );
    }

    #[test]
    fn a_jpeg_quality_outside_1_to_100_is_refused() {
        let dev = make_developed(4, 4, vec![30000u16; 4 * 4 * 3]);
        match render(&dev, None, OutputFormat::Jpeg { quality: 0 }) {
            Err(Error::Refused(_)) => {}
            other => panic!("expected Error::Refused for quality 0, got {other:?}"),
        }
        match render(&dev, None, OutputFormat::Jpeg { quality: 101 }) {
            Err(Error::Refused(_)) => {}
            other => panic!("expected Error::Refused for quality 101, got {other:?}"),
        }
        assert!(render(&dev, None, OutputFormat::Jpeg { quality: 1 }).is_ok());
        assert!(render(&dev, None, OutputFormat::Jpeg { quality: 100 }).is_ok());
    }

    #[test]
    fn the_extension_follows_the_format() {
        let w = 24;
        let h = 16;
        let dev = make_developed(w, h, vec![30000u16; (w * h * 3) as usize]);
        let res_jpeg = render(&dev, None, OutputFormat::Jpeg { quality: 95 }).unwrap();
        assert_eq!(res_jpeg.extension, "jpg");
        assert_eq!(res_jpeg.width, w);
        assert_eq!(res_jpeg.height, h);

        let res_tiff = render(&dev, None, OutputFormat::Tiff).unwrap();
        assert_eq!(res_tiff.extension, "tif");
        assert_eq!(res_tiff.width, w);
        assert_eq!(res_tiff.height, h);
    }
}
