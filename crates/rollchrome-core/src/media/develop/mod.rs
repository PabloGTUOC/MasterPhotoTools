//! Neutral RAW development pipeline (DV-1).
//!
//! Converts sensor RAW data into scene-referred linear ProPhoto RGB and 16-bit
//! ROMM-encoded ProPhoto RGB. Never uses or falls back to camera embedded previews;
//! all development is computed directly from sensor CFA data.
//!
//! Sensor CFA types supported:
//! - Bayer (2x2 RGB patterns) via rawler's PPG algorithm.
//! - Fujifilm X-Trans (6x6 RGB patterns) via our own cleanroom implementation.

use phototools_core::error::Error;
use rawler::cfa::PlaneColor;
use rawler::decoders::RawDecodeParams;
use rawler::imgop::sensor::bayer::ppg::PPGDemosaic;
use rawler::imgop::sensor::bayer::Demosaic as BayerDemosaic;
use rawler::imgop::xyz::Illuminant;
use rawler::imgop::Dim2;
use rawler::imgop::Point;
use rawler::imgop::Rect;
use rawler::pixarray::PixF32;
use rawler::rawimage::RawPhotometricInterpretation;
use rawler::rawsource::RawSource;
use rayon::prelude::*;

pub mod colour;
pub mod render;
pub mod tone;
pub mod xtrans;

pub use render::{prophoto_icc_profile, render, OutputFormat, Rendered};

/// Options controlling RAW development.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DevelopOptions {
    /// Exposure adjustment in stops (-4.0..=4.0 EV). Default: 0.0.
    pub exposure_ev: f32,
}

impl Default for DevelopOptions {
    fn default() -> Self {
        Self { exposure_ev: 0.0 }
    }
}

/// The demosaicing method selected based on sensor CFA layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Demosaic {
    Bayer,
    XTrans,
}

/// Linear, scene-referred ProPhoto RGB stage.
#[derive(Debug)]
pub struct LinearProPhoto {
    pub width: u32,
    pub height: u32,
    /// Row-major RGB pixels in ProPhoto RGB primaries.
    pub pixels: Vec<[f32; 3]>,
    /// EXIF orientation 1–8 as reported by the raw decoder.
    pub orientation: u16,
    pub demosaic: Demosaic,
}

/// Developed 16-bit ProPhoto RGB image.
pub struct Developed {
    /// 16-bit ProPhoto RGB image buffer with ROMM transfer function applied.
    pub image: image::ImageBuffer<image::Rgb<u16>, Vec<u16>>,
    pub orientation: u16,
    pub demosaic: Demosaic,
}

/// Develops raw file bytes to scene-referred linear ProPhoto RGB.
///
/// Order of operations:
/// 1. Decode bytes with rawler inside catch_unwind (never panics out).
/// 2. Validate layout (CFA cpp == 1, Bayer or X-Trans). Rejects previews.
/// 3. Rescale black and white levels to 0.0..1.0 float.
/// 4. Demosaic active sensor area.
/// 5. As-shot white balance with highlight clamp to preserve neutral clipped highlights.
/// 6. Camera matrix to linear ProPhoto RGB with negative clamping.
/// 7. Default crop.
/// 8. Exposure scale multiplication.
pub fn develop_linear(raw: &[u8], options: &DevelopOptions) -> Result<LinearProPhoto, Error> {
    if !options.exposure_ev.is_finite() || options.exposure_ev < -4.0 || options.exposure_ev > 4.0 {
        return Err(Error::Refused(format!(
            "exposure_ev {} is outside supported range -4.0..=4.0 or not finite",
            options.exposure_ev
        )));
    }

    // Step 1: Decode inside catch_unwind
    let decode_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let source = RawSource::new_from_slice(raw);
        rawler::decode(&source, &RawDecodeParams::default())
    }));

    let mut raw_image = match decode_result {
        Ok(Ok(img)) => img,
        Ok(Err(e)) => return Err(Error::Refused(format!("RAW decode failed: {}", e))),
        Err(panic_payload) => {
            let msg = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                (*s).to_string()
            } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                s.clone()
            } else {
                "unknown panic in rawler".to_string()
            };
            return Err(Error::Internal(format!("RAW decode panicked: {}", msg)));
        }
    };

    // Step 2: Validate CFA and samples per pixel (never use embedded preview)
    let cfa_config = match &raw_image.photometric {
        RawPhotometricInterpretation::LinearRaw => {
            return Err(Error::Refused(
                "unsupported RAW layout: linear DNG".to_string(),
            ));
        }
        RawPhotometricInterpretation::BlackIsZero => {
            return Err(Error::Refused(
                "unsupported RAW layout: monochrome".to_string(),
            ));
        }
        RawPhotometricInterpretation::Cfa(cfg) => cfg,
    };

    if raw_image.cpp != 1 {
        return Err(Error::Refused(format!(
            "unsupported RAW layout: {} samples per pixel (expected 1)",
            raw_image.cpp
        )));
    }

    let c = &cfa_config.cfa;
    let (cfa, demosaic) = {
        if !c.is_rgb() {
            if c.unique_colors() == 4 {
                return Err(Error::Refused(
                    "unsupported RAW layout: four-colour".to_string(),
                ));
            } else {
                return Err(Error::Refused(format!(
                    "unsupported RAW layout: non-RGB CFA pattern '{}'",
                    c
                )));
            }
        }
        if c.width == 2 && c.height == 2 {
            (c.clone(), Demosaic::Bayer)
        } else if c.width == 6 && c.height == 6 {
            (c.clone(), Demosaic::XTrans)
        } else {
            return Err(Error::Refused(format!(
                "unsupported RAW layout: CFA pattern size {}x{}",
                c.width, c.height
            )));
        }
    };

    // Step 3: Rescale sensor levels to 0.0..1.0
    raw_image
        .apply_scaling()
        .map_err(|e| Error::Refused(format!("RAW sensor scaling failed: {}", e)))?;

    // Step 4: Demosaic active area
    let full_rect = Rect::new(
        Point::new(0, 0),
        Dim2::new(raw_image.width, raw_image.height),
    );
    let active_area = raw_image.active_area.unwrap_or(full_rect);

    let pix_f32 = PixF32::new_with(
        raw_image.data.as_f32().into_owned(),
        raw_image.width,
        raw_image.height,
    );

    let demosaiced_rgb: Vec<[f32; 3]> = match demosaic {
        Demosaic::Bayer => {
            let ppg = PPGDemosaic::new();
            let c2d = ppg.demosaic(&pix_f32, &cfa, &PlaneColor::default(), active_area);
            c2d.into_inner()
        }
        Demosaic::XTrans => xtrans::demosaic(&pix_f32, &cfa, active_area),
    };

    // Step 5: As-shot white balance
    let wb = raw_image.wb_coeffs;
    if wb[0].is_nan()
        || wb[1].is_nan()
        || wb[2].is_nan()
        || wb[0] <= 0.0
        || wb[1] <= 0.0
        || wb[2] <= 0.0
    {
        return Err(Error::Refused("no white balance recorded".to_string()));
    }
    let min_wb = wb[0].min(wb[1]).min(wb[2]);
    let wb_norm = [wb[0] / min_wb, wb[1] / min_wb, wb[2] / min_wb];

    // Step 6: Camera matrix to linear ProPhoto
    let found_matrix = raw_image
        .color_matrix
        .get(&Illuminant::D65)
        .or_else(|| raw_image.color_matrix.values().next());

    let flat_mat = match found_matrix {
        Some(m) if m.len() >= 9 => m,
        _ => return Err(Error::Refused("no camera colour matrix found".to_string())),
    };
    let xyz2cam = [
        [flat_mat[0], flat_mat[1], flat_mat[2]],
        [flat_mat[3], flat_mat[4], flat_mat[5]],
        [flat_mat[6], flat_mat[7], flat_mat[8]],
    ];
    let cam2prophoto = colour::camera_to_prophoto(xyz2cam)?;

    // Transform pixels to ProPhoto in parallel
    // Clamping to at most 1.0 after white-balance keeps clipped highlights neutral:
    // a pixel clipped on sensor is 1.0 in each channel; after clamp it stays (1, 1, 1).
    let prophoto_pixels: Vec<[f32; 3]> = demosaiced_rgb
        .into_par_iter()
        .map(|p| {
            let wb_clamped = [
                (p[0] * wb_norm[0]).min(1.0),
                (p[1] * wb_norm[1]).min(1.0),
                (p[2] * wb_norm[2]).min(1.0),
            ];
            let pro = colour::mul_3x3_vec(cam2prophoto, wb_clamped);
            [pro[0].max(0.0), pro[1].max(0.0), pro[2].max(0.0)]
        })
        .collect();

    // Step 7: Default crop adapted to active area
    let (mut final_pixels, final_w, final_h) = if let Some(crop_area) = raw_image.crop_area {
        let crop = crop_area.adapt(&active_area);
        let cw = crop.width();
        let ch = crop.height();
        let aw = active_area.width();

        let cropped: Vec<[f32; 3]> = (0..ch)
            .into_par_iter()
            .flat_map(|cy| {
                let sy = crop.p.y + cy;
                let row_start = sy * aw;
                let mut row = Vec::with_capacity(cw);
                for cx in 0..cw {
                    let sx = crop.p.x + cx;
                    row.push(prophoto_pixels[row_start + sx]);
                }
                row
            })
            .collect();
        (cropped, cw as u32, ch as u32)
    } else {
        (
            prophoto_pixels,
            active_area.width() as u32,
            active_area.height() as u32,
        )
    };

    // Step 8: Exposure adjustment
    let exp_scale = 2.0f32.powf(options.exposure_ev);
    if (exp_scale - 1.0).abs() > 1e-6 {
        final_pixels.par_iter_mut().for_each(|p| {
            p[0] *= exp_scale;
            p[1] *= exp_scale;
            p[2] *= exp_scale;
        });
    }

    let ori = raw_image.orientation.to_u16();
    let orientation = if ori == 0 { 1 } else { ori };

    Ok(LinearProPhoto {
        width: final_w,
        height: final_h,
        pixels: final_pixels,
        orientation,
        demosaic,
    })
}

/// Fully develops raw file bytes into 16-bit ProPhoto RGB image.
pub fn develop(raw: &[u8], options: &DevelopOptions) -> Result<Developed, Error> {
    let linear = develop_linear(raw, options)?;
    let w = linear.width;
    let h = linear.height;

    // Apply base tone, ProPhoto ROMM transfer function, and scale to u16
    let u16_data: Vec<u16> = linear
        .pixels
        .into_par_iter()
        .flat_map(|p| {
            let mut out = [0u16; 3];
            for c in 0..3 {
                let t = tone::base_tone(p[c]);
                let enc = tone::prophoto_encode(t);
                out[c] = (enc * 65535.0).round().clamp(0.0, 65535.0) as u16;
            }
            out
        })
        .collect();

    let image = match image::ImageBuffer::from_raw(w, h, u16_data) {
        Some(img) => img,
        None => {
            return Err(Error::Internal(
                "Failed to allocate 16-bit ImageBuffer".to_string(),
            ))
        }
    };

    Ok(Developed {
        image,
        orientation: linear.orientation,
        demosaic: linear.demosaic,
    })
}

/// Converts a 16-bit ProPhoto RGB image into standard 8-bit sRGB for display.
///
/// Steps:
/// 1. ROMM-decodes 16-bit ProPhoto RGB values to linear ProPhoto floats.
/// 2. Applies 3x3 matrix conversion: ProPhoto (D50) -> XYZ -> Bradford D50->D65 -> linear sRGB.
/// 3. Clamps linear sRGB to 0.0..=1.0 (colours outside sRGB gamut are clipped).
/// 4. Applies sRGB transfer function and rounds to u8.
pub fn prophoto_to_srgb8(developed: &Developed) -> image::RgbImage {
    render::render_srgb8_image(developed, None)
}
