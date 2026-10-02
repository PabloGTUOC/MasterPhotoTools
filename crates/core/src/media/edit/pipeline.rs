//! The colour and adjustment pipeline in `core` (ED-1).
//!
//! Evaluates parametric exposure, white balance, tone crossover, contrast,
//! and vibrance/saturation in linear light, transfer-converting to/from sRGB.

use super::lut::Lut;
use crate::error::Error;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::LazyLock;

/// A portable content-addressed reference to a 3D LUT.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LutRef {
    pub name: String,
    pub sha256: String,
}

/// Parametric photographic adjustment recipe.
///
/// Default values correspond to exact identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdjustmentRecipe {
    pub version: u32,
    /// SHA-256 of the source image file at the time of recipe creation.
    pub source_sha256: String,
    /// Exposure compensation in EV stops: -5.0 to +5.0 (0.0 = identity).
    pub exposure: f32,
    /// White balance temperature shift: -100.0 to +100.0.
    pub temperature: f32,
    /// White balance tint shift: -100.0 to +100.0.
    pub tint: f32,
    /// Highlight recovery / boost: -100.0 to +100.0 (anchored, leaves 0.18 untouched).
    pub highlights: f32,
    /// Shadow lifting / crushing: -100.0 to +100.0 (anchored, leaves 0.18 untouched).
    pub shadows: f32,
    /// Contrast: -100.0 to +100.0 (power curve pivoting at 0.18, clipped at white by display transfer).
    pub contrast: f32,
    /// Global saturation: -100.0 to +100.0.
    pub saturation: f32,
    /// Vibrance (smart saturation protecting saturated tones): -100.0 to +100.0.
    pub vibrance: f32,
    /// Optional portable reference to a 3D LUT.
    pub lut: Option<LutRef>,
    /// LUT blend factor: 0.0 to 1.0 (1.0 = 100% LUT effect).
    pub lut_intensity: f32,

    // Round 2 basic additions (ED-9):
    /// Whites shoulder boost/recovery: -100.0 to +100.0 (anchored at 0.18).
    #[serde(default)]
    pub whites: f32,
    /// Blacks toe crushing/lifting: -100.0 to +100.0 (anchored at 0.18).
    #[serde(default)]
    pub blacks: f32,
    /// Mid-tone brightness power curve: -100.0 to +100.0 (pins 0.0 and 1.0, shifts 0.18).
    #[serde(default)]
    pub brightness: f32,
    /// Hue rotation in OkLCh perceptual color space: -180.0 to +180.0 degrees.
    #[serde(default)]
    pub hue: f32,

    // Round 2 structured groups (ED-10, ED-11):
    /// Optional tone curves (Luma, R, G, B) evaluated in display-encoded space.
    #[serde(default)]
    pub curves: Option<super::curves::ToneCurves>,
    /// Optional 8-band HSL adjustments evaluated in OkLCh color space.
    #[serde(default)]
    pub hsl: Option<super::hsl::HslAdjustments>,
    /// Optional 3-way colour grading wheels evaluated in display-encoded space.
    #[serde(default)]
    pub grading: Option<super::grading::ColorGrading>,

    // Round 2 geometry (ED-13):
    /// Optional geometry transforms (crop, rotate, straighten, flip) evaluated in upright frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geometry: Option<super::geometry::Geometry>,

    // Round 2 effects (ED-14):
    /// Optional scale-independent vignette evaluated in linear light post-crop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vignette: Option<super::vignette::Vignette>,
    /// Optional scale-independent film grain evaluated in display domain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grain: Option<super::grain::FilmGrain>,

    // Round 2 looks (ED-16):
    /// Optional photographic looks (glow, halation, optional tone mapper) evaluated in scene-linear light.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub looks: Option<super::looks::LookEffects>,

    // Round 3 local adjustments (ED-19):
    /// Masks, each with its own adjustments, in stored-frame coordinates (`masks`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub masks: Vec<super::masks::Mask>,
}

impl Default for AdjustmentRecipe {
    fn default() -> Self {
        Self {
            version: 3,
            source_sha256: String::new(),
            exposure: 0.0,
            temperature: 0.0,
            tint: 0.0,
            highlights: 0.0,
            shadows: 0.0,
            contrast: 0.0,
            saturation: 0.0,
            vibrance: 0.0,
            lut: None,
            lut_intensity: 1.0,
            whites: 0.0,
            blacks: 0.0,
            brightness: 0.0,
            hue: 0.0,
            curves: None,
            hsl: None,
            grading: None,
            geometry: None,
            vignette: None,
            grain: None,
            looks: None,
            masks: Vec::new(),
        }
    }
}

impl AdjustmentRecipe {
    /// True if all sliders are at rest (0.0), no curves are active, no HSL is applied, no grading is active, no geometry is active, no vignette or grain is active, no looks are active, no mask can change a pixel, and no effective LUT is attached (none or 0% intensity).
    pub fn is_identity(&self) -> bool {
        self.exposure == 0.0
            && self.temperature == 0.0
            && self.tint == 0.0
            && self.highlights == 0.0
            && self.shadows == 0.0
            && self.contrast == 0.0
            && self.saturation == 0.0
            && self.vibrance == 0.0
            && (self.lut.is_none() || self.lut_intensity == 0.0)
            && self.whites == 0.0
            && self.blacks == 0.0
            && self.brightness == 0.0
            && self.hue == 0.0
            && self.curves.as_ref().map_or(true, |c| c.is_identity())
            && self.hsl.as_ref().map_or(true, |h| h.is_identity())
            && self.grading.as_ref().map_or(true, |g| g.is_identity())
            && self.geometry.as_ref().map_or(true, |g| g.is_identity())
            && self.vignette.as_ref().map_or(true, |v| v.is_identity())
            && self.grain.as_ref().map_or(true, |g| g.is_identity())
            && self.looks.as_ref().map_or(true, |l| l.is_identity())
            && self.masks.iter().all(|m| m.is_identity())
    }
}

/// Decoded RGB image buffer (8-bit or 16-bit per channel).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageBuffer {
    Rgb8 {
        width: u32,
        height: u32,
        data: Vec<u8>,
    },
    Rgb16 {
        width: u32,
        height: u32,
        data: Vec<u16>,
    },
}

impl ImageBuffer {
    pub fn width(&self) -> u32 {
        match self {
            ImageBuffer::Rgb8 { width, .. } => *width,
            ImageBuffer::Rgb16 { width, .. } => *width,
        }
    }

    pub fn height(&self) -> u32 {
        match self {
            ImageBuffer::Rgb8 { height, .. } => *height,
            ImageBuffer::Rgb16 { height, .. } => *height,
        }
    }

    pub fn as_rgb8(&self) -> Option<&[u8]> {
        match self {
            ImageBuffer::Rgb8 { data, .. } => Some(data),
            _ => None,
        }
    }

    pub fn as_rgb16(&self) -> Option<&[u16]> {
        match self {
            ImageBuffer::Rgb16 { data, .. } => Some(data),
            _ => None,
        }
    }
}

impl From<image::DynamicImage> for ImageBuffer {
    fn from(dyn_img: image::DynamicImage) -> Self {
        match dyn_img {
            image::DynamicImage::ImageRgb16(img) => {
                let (w, h) = (img.width(), img.height());
                ImageBuffer::Rgb16 {
                    width: w,
                    height: h,
                    data: img.into_raw(),
                }
            }
            image::DynamicImage::ImageRgba16(img) => {
                let rgb = image::DynamicImage::ImageRgba16(img).into_rgb16();
                let (w, h) = (rgb.width(), rgb.height());
                ImageBuffer::Rgb16 {
                    width: w,
                    height: h,
                    data: rgb.into_raw(),
                }
            }
            image::DynamicImage::ImageLuma16(_)
            | image::DynamicImage::ImageLumaA16(_)
            | image::DynamicImage::ImageRgb32F(_)
            | image::DynamicImage::ImageRgba32F(_) => {
                let rgb = dyn_img.into_rgb16();
                let (w, h) = (rgb.width(), rgb.height());
                ImageBuffer::Rgb16 {
                    width: w,
                    height: h,
                    data: rgb.into_raw(),
                }
            }
            image::DynamicImage::ImageRgb8(img) => {
                let (w, h) = (img.width(), img.height());
                ImageBuffer::Rgb8 {
                    width: w,
                    height: h,
                    data: img.into_raw(),
                }
            }
            other => {
                let rgb = other.into_rgb8();
                let (w, h) = (rgb.width(), rgb.height());
                ImageBuffer::Rgb8 {
                    width: w,
                    height: h,
                    data: rgb.into_raw(),
                }
            }
        }
    }
}

/// Linear light `f32` floating point buffer [0.0, ∞).
#[derive(Debug, Clone, PartialEq)]
pub struct LinearBuffer {
    pub width: u32,
    pub height: u32,
    pub data: Vec<f32>,
}

impl LinearBuffer {
    pub fn new(width: u32, height: u32) -> Self {
        let size = (width as usize)
            .checked_mul(height as usize)
            .and_then(|px| px.checked_mul(3))
            .expect("image buffer size overflow");
        Self {
            width,
            height,
            data: vec![0.0; size],
        }
    }

    pub fn from_image_buffer(buffer: &ImageBuffer) -> Self {
        match buffer {
            ImageBuffer::Rgb8 {
                width,
                height,
                data,
            } => {
                let mut lin = Self::new(*width, *height);
                lin.data
                    .par_chunks_exact_mut(3)
                    .zip(data.par_chunks_exact(3))
                    .for_each(|(out_px, in_px)| {
                        out_px[0] = u8_to_linear(in_px[0]);
                        out_px[1] = u8_to_linear(in_px[1]);
                        out_px[2] = u8_to_linear(in_px[2]);
                    });
                lin
            }
            ImageBuffer::Rgb16 {
                width,
                height,
                data,
            } => {
                let mut lin = Self::new(*width, *height);
                lin.data
                    .par_chunks_exact_mut(3)
                    .zip(data.par_chunks_exact(3))
                    .for_each(|(out_px, in_px)| {
                        out_px[0] = u16_to_linear(in_px[0]);
                        out_px[1] = u16_to_linear(in_px[1]);
                        out_px[2] = u16_to_linear(in_px[2]);
                    });
                lin
            }
        }
    }

    pub fn to_rgb8(&self) -> ImageBuffer {
        self.to_rgb8_with_lut(None, 0.0)
    }

    pub fn to_rgb8_with_lut(&self, lut: Option<&Lut>, intensity: f32) -> ImageBuffer {
        self.to_rgb8_with_lut_and_curves(lut, intensity, &super::curves::ToneCurvesTable::default())
    }

    pub fn to_rgb8_with_lut_and_curves(
        &self,
        lut: Option<&Lut>,
        intensity: f32,
        curves_table: &super::curves::ToneCurvesTable,
    ) -> ImageBuffer {
        self.to_rgb8_with_lut_curves_and_grading(
            lut,
            intensity,
            curves_table,
            &super::grading::CompiledGradingTable::default(),
        )
    }

    pub fn to_rgb8_with_lut_curves_and_grading(
        &self,
        lut: Option<&Lut>,
        intensity: f32,
        curves_table: &super::curves::ToneCurvesTable,
        grading_table: &super::grading::CompiledGradingTable,
    ) -> ImageBuffer {
        self.to_rgb8_with_lut_curves_grading_and_grain(
            lut,
            intensity,
            curves_table,
            grading_table,
            &super::grain::CompiledGrain::identity(),
        )
    }

    pub fn to_rgb8_with_lut_curves_grading_and_grain(
        &self,
        lut: Option<&Lut>,
        intensity: f32,
        curves_table: &super::curves::ToneCurvesTable,
        grading_table: &super::grading::CompiledGradingTable,
        compiled_grain: &super::grain::CompiledGrain,
    ) -> ImageBuffer {
        let intensity = intensity.clamp(0.0, 1.0);
        let has_grading = !grading_table.is_identity;
        let grading_lut = &*grading_table.table;
        let has_grain = !compiled_grain.is_identity;
        let w = self.width;
        let w_f = w as f32;
        let inv_w = 1.0 / w_f.max(1.0);
        let row_len = (w * 3) as usize;
        let mut out = vec![0u8; self.data.len()];

        out.par_chunks_exact_mut(row_len)
            .zip(self.data.par_chunks_exact(row_len))
            .enumerate()
            .for_each(|(y, (out_row, in_row))| {
                let y_f = y as f32;
                let py1 = (y_f + 0.5) * inv_w * compiled_grain.k_base;
                for (x, (out_px, in_px)) in out_row
                    .chunks_exact_mut(3)
                    .zip(in_row.chunks_exact(3))
                    .enumerate()
                {
                    let disp_r = linear_to_srgb(in_px[0]);
                    let disp_g = linear_to_srgb(in_px[1]);
                    let disp_b = linear_to_srgb(in_px[2]);

                    let (curved_r, curved_g, curved_b) = if curves_table.has_any {
                        curves_table.apply(disp_r, disp_g, disp_b)
                    } else {
                        (disp_r, disp_g, disp_b)
                    };

                    let (graded_r, graded_g, graded_b) = if has_grading {
                        super::grading::CompiledGradingTable::apply_table(
                            grading_lut,
                            curved_r,
                            curved_g,
                            curved_b,
                        )
                    } else {
                        (curved_r, curved_g, curved_b)
                    };

                    let (final_r, final_g, final_b) = if let Some(lut) = lut {
                        if intensity > 0.0 {
                            let lut_out = lut.sample([graded_r, graded_g, graded_b]);
                            (
                                ((1.0 - intensity) * graded_r + intensity * lut_out[0])
                                    .clamp(0.0, 1.0),
                                ((1.0 - intensity) * graded_g + intensity * lut_out[1])
                                    .clamp(0.0, 1.0),
                                ((1.0 - intensity) * graded_b + intensity * lut_out[2])
                                    .clamp(0.0, 1.0),
                            )
                        } else {
                            (graded_r, graded_g, graded_b)
                        }
                    } else {
                        (graded_r, graded_g, graded_b)
                    };

                    let (grain_r, grain_g, grain_b) = if has_grain {
                        let u = (x as f32 + 0.5) * inv_w;
                        let delta =
                            compiled_grain.sample_delta_with_v(u, py1, final_r, final_g, final_b);
                        (
                            (final_r + delta).clamp(0.0, 1.0),
                            (final_g + delta).clamp(0.0, 1.0),
                            (final_b + delta).clamp(0.0, 1.0),
                        )
                    } else {
                        (final_r, final_g, final_b)
                    };

                    out_px[0] = (grain_r * 255.0).round().clamp(0.0, 255.0) as u8;
                    out_px[1] = (grain_g * 255.0).round().clamp(0.0, 255.0) as u8;
                    out_px[2] = (grain_b * 255.0).round().clamp(0.0, 255.0) as u8;
                }
            });
        ImageBuffer::Rgb8 {
            width: self.width,
            height: self.height,
            data: out,
        }
    }

    pub fn to_rgb16(&self) -> ImageBuffer {
        self.to_rgb16_with_lut(None, 0.0)
    }

    pub fn to_rgb16_with_lut(&self, lut: Option<&Lut>, intensity: f32) -> ImageBuffer {
        self.to_rgb16_with_lut_and_curves(
            lut,
            intensity,
            &super::curves::ToneCurvesTable::default(),
        )
    }

    pub fn to_rgb16_with_lut_and_curves(
        &self,
        lut: Option<&Lut>,
        intensity: f32,
        curves_table: &super::curves::ToneCurvesTable,
    ) -> ImageBuffer {
        self.to_rgb16_with_lut_curves_and_grading(
            lut,
            intensity,
            curves_table,
            &super::grading::CompiledGradingTable::default(),
        )
    }

    pub fn to_rgb16_with_lut_curves_and_grading(
        &self,
        lut: Option<&Lut>,
        intensity: f32,
        curves_table: &super::curves::ToneCurvesTable,
        grading_table: &super::grading::CompiledGradingTable,
    ) -> ImageBuffer {
        self.to_rgb16_with_lut_curves_grading_and_grain(
            lut,
            intensity,
            curves_table,
            grading_table,
            &super::grain::CompiledGrain::identity(),
        )
    }

    pub fn to_rgb16_with_lut_curves_grading_and_grain(
        &self,
        lut: Option<&Lut>,
        intensity: f32,
        curves_table: &super::curves::ToneCurvesTable,
        grading_table: &super::grading::CompiledGradingTable,
        compiled_grain: &super::grain::CompiledGrain,
    ) -> ImageBuffer {
        let intensity = intensity.clamp(0.0, 1.0);
        let has_grading = !grading_table.is_identity;
        let grading_lut = &*grading_table.table;
        let has_grain = !compiled_grain.is_identity;
        let w = self.width;
        let w_f = w as f32;
        let inv_w = 1.0 / w_f.max(1.0);
        let row_len = (w * 3) as usize;
        let mut out = vec![0u16; self.data.len()];

        out.par_chunks_exact_mut(row_len)
            .zip(self.data.par_chunks_exact(row_len))
            .enumerate()
            .for_each(|(y, (out_row, in_row))| {
                let y_f = y as f32;
                let py1 = (y_f + 0.5) * inv_w * compiled_grain.k_base;
                for (x, (out_px, in_px)) in out_row
                    .chunks_exact_mut(3)
                    .zip(in_row.chunks_exact(3))
                    .enumerate()
                {
                    let disp_r = linear_to_srgb(in_px[0]);
                    let disp_g = linear_to_srgb(in_px[1]);
                    let disp_b = linear_to_srgb(in_px[2]);

                    let (curved_r, curved_g, curved_b) = if curves_table.has_any {
                        curves_table.apply(disp_r, disp_g, disp_b)
                    } else {
                        (disp_r, disp_g, disp_b)
                    };

                    let (graded_r, graded_g, graded_b) = if has_grading {
                        super::grading::CompiledGradingTable::apply_table(
                            grading_lut,
                            curved_r,
                            curved_g,
                            curved_b,
                        )
                    } else {
                        (curved_r, curved_g, curved_b)
                    };

                    let (final_r, final_g, final_b) = if let Some(lut) = lut {
                        if intensity > 0.0 {
                            let lut_out = lut.sample([graded_r, graded_g, graded_b]);
                            (
                                ((1.0 - intensity) * graded_r + intensity * lut_out[0])
                                    .clamp(0.0, 1.0),
                                ((1.0 - intensity) * graded_g + intensity * lut_out[1])
                                    .clamp(0.0, 1.0),
                                ((1.0 - intensity) * graded_b + intensity * lut_out[2])
                                    .clamp(0.0, 1.0),
                            )
                        } else {
                            (graded_r, graded_g, graded_b)
                        }
                    } else {
                        (graded_r, graded_g, graded_b)
                    };

                    let (grain_r, grain_g, grain_b) = if has_grain {
                        let u = (x as f32 + 0.5) * inv_w;
                        let delta =
                            compiled_grain.sample_delta_with_v(u, py1, final_r, final_g, final_b);
                        (
                            (final_r + delta).clamp(0.0, 1.0),
                            (final_g + delta).clamp(0.0, 1.0),
                            (final_b + delta).clamp(0.0, 1.0),
                        )
                    } else {
                        (final_r, final_g, final_b)
                    };

                    out_px[0] = (grain_r * 65535.0).round().clamp(0.0, 65535.0) as u16;
                    out_px[1] = (grain_g * 65535.0).round().clamp(0.0, 65535.0) as u16;
                    out_px[2] = (grain_b * 65535.0).round().clamp(0.0, 65535.0) as u16;
                }
            });
        ImageBuffer::Rgb16 {
            width: self.width,
            height: self.height,
            data: out,
        }
    }

    /// Evaluates the pipeline stages in linear light in the specified order:
    /// white balance → exposure → highlights/shadows → contrast → saturation/vibrance.
    pub fn apply_adjustments(&mut self, recipe: &AdjustmentRecipe) {
        self.apply_adjustments_masked(recipe, None);
    }

    /// As `apply_adjustments`, with each pixel's adjustments raised by the masks that cover
    /// it (ED-19). This buffer is the geometry's output, so `masks` must be compiled against
    /// the same geometry. A pixel no mask covers takes exactly the global arithmetic.
    pub fn apply_adjustments_masked(
        &mut self,
        recipe: &AdjustmentRecipe,
        masks: Option<&super::masks::CompiledMasks>,
    ) {
        let global = LinearParams::new(recipe, &super::masks::LocalAdjustments::default());
        let touches = |f: fn(&super::masks::CompiledMasks) -> bool| masks.is_some_and(f);
        let flags = StageFlags {
            tone: global.has_tone || touches(|m| m.touches_tone),
            contrast: global.has_contrast || touches(|m| m.touches_contrast),
            sat_vib: global.has_sat_vib || touches(|m| m.touches_saturation),
        };

        let compiled_hsl =
            super::hsl::CompiledHslTable::from_recipe(recipe.hsl.as_ref(), recipe.hue);
        let has_oklab = !compiled_hsl.is_identity;
        let (w, h) = (self.width, self.height);

        self.data
            .par_chunks_exact_mut((w as usize) * 3)
            .enumerate()
            .for_each(|(y, row)| {
                let cursor = masks.map(|m| (m, m.row(y as u32, w, h)));
                for (x, px) in row.chunks_exact_mut(3).enumerate() {
                    let local_params;
                    let p = match cursor {
                        Some((m, c)) => {
                            let (mx, my) = c.at(x);
                            let local = m.local_at(mx, my);
                            if local.is_zero() {
                                &global
                            } else {
                                local_params = LinearParams::new(recipe, &local);
                                &local_params
                            }
                        }
                        None => &global,
                    };
                    let (mut r, mut g, mut b) = p.apply(px[0], px[1], px[2], &flags);

                    // Oklab stage: Global Hue rotation and 8-band HSL in ONE pass
                    if has_oklab {
                        let (hr, hg, hb) = compiled_hsl.apply_linear_srgb(r, g, b);
                        r = hr;
                        g = hg;
                        b = hb;
                    }

                    px[0] = r;
                    px[1] = g;
                    px[2] = b;
                }
            });
    }
}

/// Which of the optional stages run for this image: on when the global value or any mask
/// asks for it, so a stage at rest globally still runs where a mask is active.
#[derive(Clone, Copy)]
struct StageFlags {
    tone: bool,
    contrast: bool,
    sat_vib: bool,
}

/// The per-pixel constants of the export pass, from the global recipe plus a mask's local
/// adjustments (zero for the global set). One evaluation path for both, so a masked pixel
/// and an unmasked one are adjusted by the same arithmetic.
struct LinearParams {
    gain_r: f32,
    gain_g: f32,
    gain_b: f32,
    h: f32,
    s: f32,
    w: f32,
    b_val: f32,
    has_tone: bool,
    has_brightness: bool,
    bright_exp: f32,
    has_contrast: bool,
    gamma_minus_one: f32,
    sat: f32,
    vib: f32,
    has_sat_vib: bool,
}

impl LinearParams {
    fn new(recipe: &AdjustmentRecipe, local: &super::masks::LocalAdjustments) -> Self {
        // Precompute channel gains and flags once outside the pixel loop
        let t = (recipe.temperature + local.temperature) / 100.0;
        let g = (recipe.tint + local.tint) / 100.0;
        let wb_r = 2.0f32.powf(0.5 * t);
        let wb_b = 2.0f32.powf(-0.5 * t);
        let wb_g = 2.0f32.powf(-0.5 * g);
        let exp_gain = 2.0f32.powf(recipe.exposure + local.exposure);

        let h = (recipe.highlights + local.highlights) / 100.0;
        let s = (recipe.shadows + local.shadows) / 100.0;
        let w = (recipe.whites + local.whites) / 100.0;
        let b_val = (recipe.blacks + local.blacks) / 100.0;

        let bright = recipe.brightness / 100.0;
        let c = (recipe.contrast + local.contrast) / 100.0;
        let sat = (recipe.saturation + local.saturation) / 100.0;
        let vib = (recipe.vibrance + local.vibrance) / 100.0;

        Self {
            gain_r: wb_r * exp_gain,
            gain_g: wb_g * exp_gain,
            gain_b: wb_b * exp_gain,
            h,
            s,
            w,
            b_val,
            has_tone: h != 0.0 || s != 0.0 || w != 0.0 || b_val != 0.0,
            has_brightness: bright != 0.0,
            bright_exp: 2.0f32.powf(-0.5 * bright) - 1.0,
            has_contrast: c != 0.0,
            gamma_minus_one: 0.5 * c,
            sat,
            vib,
            has_sat_vib: sat != 0.0 || vib != 0.0,
        }
    }

    #[inline(always)]
    fn apply(&self, r: f32, g: f32, b: f32, flags: &StageFlags) -> (f32, f32, f32) {
        // 1 & 2. White Balance & Exposure
        let mut r = r * self.gain_r;
        let mut g = g * self.gain_g;
        let mut b = b * self.gain_b;

        // 3. Highlights, Shadows, Whites, Blacks (crossover at 0.18, 0.18 strictly stationary)
        if flags.tone {
            let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
            let (w_h, w_s) = tone_weights(y);
            let (w_w, w_b) = white_black_weights(y);
            let delta_ev = w_h * self.h + w_s * self.s + w_w * self.w + w_b * self.b_val;
            if delta_ev != 0.0 {
                let factor = delta_ev.exp2();
                r *= factor;
                g *= factor;
                b *= factor;
            }
        }

        // Brightness (mid-tone power curve pivoting on mid-grey, keeping 0.0 and 1.0 pinned)
        if self.has_brightness {
            let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
            if y > 1e-7 && (y - 1.0).abs() > 1e-7 {
                let factor = y.powf(self.bright_exp);
                r *= factor;
                g *= factor;
                b *= factor;
            }
        }

        // 4. Contrast (power curve pivoting at 0.18)
        if flags.contrast && self.has_contrast {
            let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
            if y > 1e-7 {
                let factor = (y / 0.18).powf(self.gamma_minus_one);
                r *= factor;
                g *= factor;
                b *= factor;
            }
        }

        // 5. Saturation & Vibrance (preserves neutral grey)
        if flags.sat_vib && self.has_sat_vib {
            let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
            let max = r.max(g).max(b);
            let min = r.min(g).min(b);
            let current_sat = if max > 1e-7 {
                ((max - min) / max).min(1.0)
            } else {
                0.0
            };
            let k = ((1.0 + self.sat) * (1.0 + self.vib * (1.0 - current_sat))).max(0.0);
            r = (y + k * (r - y)).max(0.0);
            g = (y + k * (g - y)).max(0.0);
            b = (y + k * (b - y)).max(0.0);
        }

        (r, g, b)
    }
}

/// Convert sRGB code in [0.0, 1.0] to linear light.
#[inline]
pub fn srgb_to_linear(s: f32) -> f32 {
    let s = s.clamp(0.0, 1.0);
    if s <= 0.04045 {
        s / 12.92
    } else {
        ((s + 0.055) / 1.055).powf(2.4)
    }
}

/// Convert linear light [0.0, 1.0] to sRGB code.
#[inline]
pub fn linear_to_srgb(l: f32) -> f32 {
    let l = l.clamp(0.0, 1.0);
    if l <= 0.0031308 {
        l * 12.92
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    }
}

/// 256-entry lookup table mapping 8-bit sRGB code to linear light float.
pub static SRGB_TO_LINEAR_U8: LazyLock<[f32; 256]> = LazyLock::new(|| {
    let mut table = [0.0f32; 256];
    for (i, item) in table.iter_mut().enumerate() {
        *item = srgb_to_linear(i as f32 / 255.0);
    }
    table
});

/// 65,536-entry lookup table mapping 16-bit sRGB code to linear light float.
pub static SRGB_TO_LINEAR_U16: LazyLock<Box<[f32; 65536]>> = LazyLock::new(|| {
    let mut table = vec![0.0f32; 65536];
    for (i, item) in table.iter_mut().enumerate() {
        *item = srgb_to_linear(i as f32 / 65535.0);
    }
    table.into_boxed_slice().try_into().unwrap()
});

#[inline]
pub fn u8_to_linear(v: u8) -> f32 {
    SRGB_TO_LINEAR_U8[v as usize]
}

#[inline]
pub fn linear_to_u8(l: f32) -> u8 {
    (linear_to_srgb(l) * 255.0).round().clamp(0.0, 255.0) as u8
}

#[inline]
pub fn u16_to_linear(v: u16) -> f32 {
    SRGB_TO_LINEAR_U16[v as usize]
}

#[inline]
pub fn linear_to_u16(l: f32) -> u16 {
    (linear_to_srgb(l) * 65535.0).round().clamp(0.0, 65535.0) as u16
}

/// Highlights and shadows weights for linear luminance `y`.
///
/// Guarantees that at `y = 0.18`, both weights are identically 0.0.
#[inline]
pub fn tone_weights(y: f32) -> (f32, f32) {
    let w_h = if y <= 0.18 {
        0.0
    } else if y >= 0.65 {
        1.0
    } else {
        let t = (y - 0.18) / (0.65 - 0.18);
        t * t * (3.0 - 2.0 * t)
    };

    let w_s = if y >= 0.18 {
        0.0
    } else if y <= 0.05 {
        1.0
    } else {
        let t = (0.18 - y) / (0.18 - 0.05);
        t * t * (3.0 - 2.0 * t)
    };

    (w_h, w_s)
}

/// Whites and blacks weights for linear luminance `y`.
///
/// Guarantees that at `y = 0.18`, both weights are identically 0.0.
/// Whites smoothly ramps up above 0.18 towards 1.0 at y >= 1.0.
/// Blacks smoothly ramps up below 0.18 towards 1.0 at y <= 0.02.
#[inline]
pub fn white_black_weights(y: f32) -> (f32, f32) {
    let w_w = if y <= 0.18 {
        0.0
    } else if y >= 1.0 {
        1.0
    } else {
        let t = (y - 0.18) / (1.0 - 0.18);
        t * t * (3.0 - 2.0 * t)
    };

    let w_b = if y >= 0.18 {
        0.0
    } else if y <= 0.02 {
        1.0
    } else {
        let t = (0.18 - y) / (0.18 - 0.02);
        t * t * (3.0 - 2.0 * t)
    };

    (w_w, w_b)
}

/// Validates that if a recipe specifies a LUT, the provided LUT matches its sha256,
/// and that if no LUT is specified, no LUT was provided.
pub fn validate_lut(recipe: &AdjustmentRecipe, lut: Option<&Lut>) -> Result<(), Error> {
    match (&recipe.lut, lut) {
        (Some(lut_ref), Some(provided_lut)) => {
            if provided_lut.sha256 != lut_ref.sha256 {
                return Err(Error::Refused(format!(
                    "LUT sha256 mismatch for '{}': recipe expected {}, but provided LUT has {}",
                    lut_ref.name, lut_ref.sha256, provided_lut.sha256
                )));
            }
        }
        (Some(lut_ref), None) => {
            return Err(Error::Refused(format!(
                "recipe requires 3D LUT '{}' ({}), but no LUT was provided",
                lut_ref.name, lut_ref.sha256
            )));
        }
        (None, Some(_)) => {
            return Err(Error::Refused(
                "a LUT was provided but the recipe does not specify any LUT".to_string(),
            ));
        }
        (None, None) => {}
    }
    Ok(())
}

/// Applies an adjustment recipe to an image buffer, optionally evaluating a 3D LUT.
///
/// In ED-2:
/// - If `recipe.lut` is set, `lut` must be provided and its sha256 must match `recipe.lut.sha256`.
/// - If `recipe.lut` is none, providing a `lut` is refused to avoid silent omission.
/// - The 3D LUT is evaluated in display-encoded space (after sRGB transfer and clamp,
///   before quantisation) and blended as `out = (1 - i) * disp + i * lut`.
pub fn apply_recipe(
    image: &ImageBuffer,
    recipe: &AdjustmentRecipe,
    lut: Option<&Lut>,
) -> Result<ImageBuffer, Error> {
    apply_recipe_with_orientation(image, recipe, lut, 1)
}

/// Applies an adjustment recipe with EXIF orientation handoff.
///
/// If `recipe.geometry` is active, the image is transformed and orientation is baked
/// into the upright frame. Otherwise, processing remains in stored orientation.
pub fn apply_recipe_with_orientation(
    image: &ImageBuffer,
    recipe: &AdjustmentRecipe,
    lut: Option<&Lut>,
    orientation: u32,
) -> Result<ImageBuffer, Error> {
    validate_lut(recipe, lut)?;

    let mut linear = LinearBuffer::from_image_buffer(image);
    let masks = super::masks::CompiledMasks::compile(
        &recipe.masks,
        recipe.geometry.as_ref(),
        linear.width,
        linear.height,
        orientation,
    )?;
    if let Some(geom) = &recipe.geometry {
        linear = geom.apply_to_linear(&linear, orientation)?;
    }
    linear.apply_adjustments_masked(recipe, masks.as_ref());
    if let Some(vignette) = &recipe.vignette {
        super::vignette::apply_vignette(&mut linear, vignette);
    }
    if let Some(looks) = &recipe.looks {
        super::looks::apply_looks_linear(&mut linear, looks);
    }

    let intensity = if recipe.lut.is_some() {
        recipe.lut_intensity
    } else {
        0.0
    };

    let curves_table = super::curves::ToneCurvesTable::from_recipe(recipe.curves.as_ref());
    let grading_table = super::grading::CompiledGradingTable::from_recipe(recipe.grading.as_ref());
    let compiled_grain = recipe
        .grain
        .as_ref()
        .map(|g| g.compile(&recipe.source_sha256))
        .unwrap_or_else(super::grain::CompiledGrain::identity);

    match image {
        ImageBuffer::Rgb8 { .. } => Ok(linear.to_rgb8_with_lut_curves_grading_and_grain(
            lut,
            intensity,
            &curves_table,
            &grading_table,
            &compiled_grain,
        )),
        ImageBuffer::Rgb16 { .. } => Ok(linear.to_rgb16_with_lut_curves_grading_and_grain(
            lut,
            intensity,
            &curves_table,
            &grading_table,
            &compiled_grain,
        )),
    }
}

/// Decodes an image from disk.
///
/// JPEGs and TIFFs (8-bit and 16-bit) are decoded via `image`.
/// RAW files are decoded through `media::raw::raw_to_jpeg` (the F14 ladder),
/// preserving modularity since `media` must never depend on `ingest`.
pub fn decode_image(path: &Path) -> Result<ImageBuffer, Error> {
    if crate::media::raw::is_raw(path) {
        let derived = crate::media::raw::raw_to_jpeg(path)?;
        let dyn_img = image::load_from_memory(&derived.bytes)
            .map_err(|e| Error::Internal(format!("failed to decode derived RAW JPEG: {e}")))?;
        return Ok(ImageBuffer::from(dyn_img));
    }

    let reader = image::ImageReader::open(path)?
        .with_guessed_format()
        .map_err(|e| Error::Internal(format!("could not identify {}: {e}", path.display())))?;
    let dyn_img = reader
        .decode()
        .map_err(|e| Error::Internal(format!("failed to decode {}: {e}", path.display())))?;

    Ok(ImageBuffer::from(dyn_img))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_untouched_v2_recipe_is_exact_identity() {
        // The default is now version 3 (ED-19); a v2 recipe at rest is still identity.
        let default_recipe = AdjustmentRecipe {
            version: 2,
            ..Default::default()
        };
        assert!(default_recipe.is_identity());
        assert!(AdjustmentRecipe::default().is_identity());

        // 8-bit buffer: exercise various levels and shades
        let w = 8;
        let h = 8;
        let mut data8 = Vec::with_capacity((w * h * 3) as usize);
        for i in 0..(w * h) {
            let r = ((i * 37) % 256) as u8;
            let g = ((i * 73) % 256) as u8;
            let b = ((i * 109) % 256) as u8;
            data8.extend_from_slice(&[r, g, b]);
        }
        let buf8 = ImageBuffer::Rgb8 {
            width: w,
            height: h,
            data: data8,
        };
        let out8 = apply_recipe(&buf8, &default_recipe, None).unwrap();
        assert_eq!(
            out8, buf8,
            "8-bit untouched recipe must be bit-exact identity"
        );

        // 16-bit buffer: exercise various levels
        let mut data16 = Vec::with_capacity((w * h * 3) as usize);
        for i in 0..(w * h) {
            let r = ((i * 10007) % 65536) as u16;
            let g = ((i * 20011) % 65536) as u16;
            let b = ((i * 30013) % 65536) as u16;
            data16.extend_from_slice(&[r, g, b]);
        }
        let buf16 = ImageBuffer::Rgb16 {
            width: w,
            height: h,
            data: data16,
        };
        let out16 = apply_recipe(&buf16, &default_recipe, None).unwrap();
        assert_eq!(
            out16, buf16,
            "16-bit untouched recipe must be bit-exact identity"
        );
    }

    #[test]
    fn srgb_round_trips_every_8_bit_code_exactly() {
        for code in 0..=255u8 {
            let lin = u8_to_linear(code);
            let recovered = linear_to_u8(lin);
            assert_eq!(recovered, code, "failed round-trip for 8-bit code {code}");
        }
    }

    #[test]
    fn srgb_round_trips_every_16_bit_code_exactly() {
        for code in 0..=65535u16 {
            let lin = u16_to_linear(code);
            let recovered = linear_to_u16(lin);
            assert_eq!(recovered, code, "failed round-trip for 16-bit code {code}");
        }
    }

    #[test]
    fn exposure_plus_one_doubles_linear_luminance() {
        let mut lin = LinearBuffer {
            width: 2,
            height: 1,
            data: vec![0.18, 0.18, 0.18, 0.10, 0.20, 0.30],
        };
        let y0_before = 0.2126 * lin.data[0] + 0.7152 * lin.data[1] + 0.0722 * lin.data[2];
        let y1_before = 0.2126 * lin.data[3] + 0.7152 * lin.data[4] + 0.0722 * lin.data[5];

        let recipe = AdjustmentRecipe {
            exposure: 1.0,
            ..AdjustmentRecipe::default()
        };
        lin.apply_adjustments(&recipe);

        let y0_after = 0.2126 * lin.data[0] + 0.7152 * lin.data[1] + 0.0722 * lin.data[2];
        let y1_after = 0.2126 * lin.data[3] + 0.7152 * lin.data[4] + 0.0722 * lin.data[5];

        assert!((y0_after - 2.0 * y0_before).abs() < 1e-6);
        assert!((y1_after - 2.0 * y1_before).abs() < 1e-6);
    }

    #[test]
    fn highlights_and_shadows_leave_mid_grey_untouched() {
        let mid_grey = 0.18f32;
        for highlights in [-100.0f32, 100.0f32] {
            for shadows in [-100.0f32, 100.0f32] {
                let mut lin = LinearBuffer {
                    width: 1,
                    height: 1,
                    data: vec![mid_grey, mid_grey, mid_grey],
                };
                let recipe = AdjustmentRecipe {
                    highlights,
                    shadows,
                    ..AdjustmentRecipe::default()
                };
                lin.apply_adjustments(&recipe);
                assert_eq!(
                    lin.data[0], mid_grey,
                    "highlights {highlights}, shadows {shadows} modified mid-grey R"
                );
                assert_eq!(
                    lin.data[1], mid_grey,
                    "highlights {highlights}, shadows {shadows} modified mid-grey G"
                );
                assert_eq!(
                    lin.data[2], mid_grey,
                    "highlights {highlights}, shadows {shadows} modified mid-grey B"
                );
            }
        }
    }

    #[test]
    fn contrast_leaves_mid_grey_untouched() {
        let mid_grey = 0.18f32;
        for contrast in [-100.0f32, -50.0f32, 0.0f32, 50.0f32, 100.0f32] {
            let mut lin = LinearBuffer {
                width: 1,
                height: 1,
                data: vec![mid_grey, mid_grey, mid_grey],
            };
            let recipe = AdjustmentRecipe {
                contrast,
                ..AdjustmentRecipe::default()
            };
            lin.apply_adjustments(&recipe);
            assert_eq!(
                lin.data[0], mid_grey,
                "contrast {contrast} modified mid-grey R"
            );
            assert_eq!(
                lin.data[1], mid_grey,
                "contrast {contrast} modified mid-grey G"
            );
            assert_eq!(
                lin.data[2], mid_grey,
                "contrast {contrast} modified mid-grey B"
            );
        }
    }

    #[test]
    fn neutral_grey_stays_neutral_with_white_balance_and_saturation_at_rest() {
        for grey_u8 in [0u8, 30, 80, 119, 128, 200, 255] {
            let img = ImageBuffer::Rgb8 {
                width: 1,
                height: 1,
                data: vec![grey_u8, grey_u8, grey_u8],
            };
            for exposure in [-2.0f32, 0.0, 1.5] {
                for contrast in [-50.0f32, 0.0, 50.0] {
                    let recipe = AdjustmentRecipe {
                        exposure,
                        contrast,
                        temperature: 0.0,
                        tint: 0.0,
                        saturation: 0.0,
                        vibrance: 0.0,
                        ..AdjustmentRecipe::default()
                    };
                    let out = apply_recipe(&img, &recipe, None).unwrap();
                    let ImageBuffer::Rgb8 { data, .. } = out else {
                        panic!("expected ImageBuffer::Rgb8, got {out:?}");
                    };
                    assert_eq!(data[0], data[1], "R != G for grey {grey_u8}");
                    assert_eq!(data[1], data[2], "G != B for grey {grey_u8}");
                }
            }
        }
    }

    #[test]
    fn a_recipe_naming_a_lut_is_refused_without_that_lut() {
        let img = ImageBuffer::Rgb8 {
            width: 1,
            height: 1,
            data: vec![128, 128, 128],
        };
        let recipe = AdjustmentRecipe {
            lut: Some(LutRef {
                name: "film_stock.cube".into(),
                sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
            }),
            ..AdjustmentRecipe::default()
        };
        let err = apply_recipe(&img, &recipe, None).unwrap_err();
        match err {
            Error::Refused(msg) => {
                assert!(
                    msg.contains("film_stock.cube"),
                    "expected LUT name, got: {msg}"
                );
                assert!(
                    msg.contains("no LUT was provided"),
                    "expected missing LUT explanation, got: {msg}"
                );
            }
            other => panic!("expected Error::Refused, got {other:?}"),
        }
    }

    #[test]
    fn a_recipe_naming_a_lut_is_refused_with_a_different_lut() {
        let img = ImageBuffer::Rgb8 {
            width: 1,
            height: 1,
            data: vec![128, 128, 128],
        };
        let cube_str = "TITLE \"Test\"\nLUT_3D_SIZE 2\n0.0 0.0 0.0\n1.0 0.0 0.0\n0.0 1.0 0.0\n1.0 1.0 0.0\n0.0 0.0 1.0\n1.0 0.0 1.0\n0.0 1.0 1.0\n1.0 1.0 1.0\n";
        let lut = Lut::from_cube("actual.cube", cube_str.as_bytes()).unwrap();

        let recipe = AdjustmentRecipe {
            lut: Some(LutRef {
                name: "expected.cube".into(),
                sha256: "different_hash_that_does_not_match".into(),
            }),
            ..AdjustmentRecipe::default()
        };
        let err = apply_recipe(&img, &recipe, Some(&lut)).unwrap_err();
        match err {
            Error::Refused(msg) => {
                assert!(
                    msg.contains("expected.cube"),
                    "expected LUT name, got: {msg}"
                );
                assert!(
                    msg.contains("mismatch"),
                    "expected mismatch explanation, got: {msg}"
                );
            }
            other => panic!("expected Error::Refused, got {other:?}"),
        }
    }

    #[test]
    fn a_recipe_without_a_lut_is_refused_when_a_lut_is_provided() {
        let img = ImageBuffer::Rgb8 {
            width: 1,
            height: 1,
            data: vec![128, 128, 128],
        };
        let cube_str = "TITLE \"Test\"\nLUT_3D_SIZE 2\n0.0 0.0 0.0\n1.0 0.0 0.0\n0.0 1.0 0.0\n1.0 1.0 0.0\n0.0 0.0 1.0\n1.0 0.0 1.0\n0.0 1.0 1.0\n1.0 1.0 1.0\n";
        let lut = Lut::from_cube("actual.cube", cube_str.as_bytes()).unwrap();

        let recipe = AdjustmentRecipe {
            lut: None,
            ..AdjustmentRecipe::default()
        };
        let err = apply_recipe(&img, &recipe, Some(&lut)).unwrap_err();
        match err {
            Error::Refused(msg) => {
                assert!(
                    msg.contains("recipe does not specify any LUT"),
                    "expected rejection of unexpected LUT, got: {msg}"
                );
            }
            other => panic!("expected Error::Refused, got {other:?}"),
        }
    }

    #[test]
    fn a_lut_is_sampled_with_display_encoded_values_not_linear_ones() {
        // Size 3 cube where nodes with r, g, b >= 0.5 (indices >= 1) output white [1.0, 1.0, 1.0],
        // while (0, 0, 0) outputs black [0.0, 0.0, 0.0].
        let mut cube_str = String::from("TITLE \"DispVsLin\"\nLUT_3D_SIZE 3\n");
        for b in 0..3 {
            for g in 0..3 {
                for r in 0..3 {
                    if r >= 1 && g >= 1 && b >= 1 {
                        cube_str.push_str("1.0 1.0 1.0\n");
                    } else {
                        cube_str.push_str("0.0 0.0 0.0\n");
                    }
                }
            }
        }
        let lut = Lut::from_cube("step.cube", cube_str.as_bytes()).unwrap();

        // 8-bit input grey 128:
        // In display sRGB space: 128/255 ≈ 0.50196 >= 0.5 on all channels.
        // In linear light space: srgb_to_linear(128/255) ≈ 0.21586 < 0.5.
        let img = ImageBuffer::Rgb8 {
            width: 1,
            height: 1,
            data: vec![128, 128, 128],
        };
        let recipe = AdjustmentRecipe {
            lut: Some(LutRef {
                name: "step.cube".into(),
                sha256: lut.sha256.clone(),
            }),
            lut_intensity: 1.0,
            ..AdjustmentRecipe::default()
        };

        let out = apply_recipe(&img, &recipe, Some(&lut)).unwrap();
        let ImageBuffer::Rgb8 { data, .. } = out else {
            panic!("expected Rgb8")
        };

        // If sampled in display space (0.502), the output must be full white [255, 255, 255].
        // If it were mistakenly sampled in linear space (0.216), it would be dark grey ~110.
        assert_eq!(
            data,
            vec![255, 255, 255],
            "LUT must be sampled in display-encoded space"
        );
    }

    #[test]
    fn lut_intensity_zero_returns_pre_lut_image() {
        let img = ImageBuffer::Rgb8 {
            width: 2,
            height: 2,
            data: vec![50, 100, 150, 200, 220, 240, 10, 80, 120, 180, 70, 90],
        };

        // Inverting LUT (out = 1.0 - in)
        let mut cube_str = String::from("TITLE \"Invert\"\nLUT_3D_SIZE 2\n");
        for b in 0..2 {
            for g in 0..2 {
                for r in 0..2 {
                    cube_str.push_str(&format!(
                        "{:.1} {:.1} {:.1}\n",
                        1.0 - r as f32,
                        1.0 - g as f32,
                        1.0 - b as f32
                    ));
                }
            }
        }
        let lut = Lut::from_cube("invert.cube", cube_str.as_bytes()).unwrap();

        let base_recipe = AdjustmentRecipe {
            exposure: 0.5,
            contrast: 20.0,
            highlights: -15.0,
            ..AdjustmentRecipe::default()
        };

        let pre_lut_out = apply_recipe(&img, &base_recipe, None).unwrap();

        let recipe_with_zero_lut = AdjustmentRecipe {
            lut: Some(LutRef {
                name: "invert.cube".into(),
                sha256: lut.sha256.clone(),
            }),
            lut_intensity: 0.0,
            ..base_recipe.clone()
        };

        let out_zero = apply_recipe(&img, &recipe_with_zero_lut, Some(&lut)).unwrap();
        assert_eq!(
            out_zero, pre_lut_out,
            "lut_intensity 0.0 must return identical image to no LUT"
        );
    }

    #[test]
    fn lut_intensity_half_blends_fifty_percent() {
        let img = ImageBuffer::Rgb8 {
            width: 1,
            height: 1,
            data: vec![100, 150, 200],
        };

        // Inverting LUT
        let mut cube_str = String::from("TITLE \"Invert\"\nLUT_3D_SIZE 2\n");
        for b in 0..2 {
            for g in 0..2 {
                for r in 0..2 {
                    cube_str.push_str(&format!(
                        "{:.1} {:.1} {:.1}\n",
                        1.0 - r as f32,
                        1.0 - g as f32,
                        1.0 - b as f32
                    ));
                }
            }
        }
        let lut = Lut::from_cube("invert.cube", cube_str.as_bytes()).unwrap();

        let recipe_zero = AdjustmentRecipe {
            lut: Some(LutRef {
                name: "invert.cube".into(),
                sha256: lut.sha256.clone(),
            }),
            lut_intensity: 0.0,
            ..AdjustmentRecipe::default()
        };
        let out_zero = apply_recipe(&img, &recipe_zero, Some(&lut)).unwrap();
        let ImageBuffer::Rgb8 {
            data: data_zero, ..
        } = out_zero
        else {
            panic!("expected Rgb8")
        };

        let recipe_full = AdjustmentRecipe {
            lut_intensity: 1.0,
            ..recipe_zero.clone()
        };
        let out_full = apply_recipe(&img, &recipe_full, Some(&lut)).unwrap();
        let ImageBuffer::Rgb8 {
            data: data_full, ..
        } = out_full
        else {
            panic!("expected Rgb8")
        };

        let recipe_half = AdjustmentRecipe {
            lut_intensity: 0.5,
            ..recipe_zero.clone()
        };
        let out_half = apply_recipe(&img, &recipe_half, Some(&lut)).unwrap();
        let ImageBuffer::Rgb8 {
            data: data_half, ..
        } = out_half
        else {
            panic!("expected Rgb8")
        };

        for c in 0..3 {
            let expected_half = ((data_zero[c] as f32 + data_full[c] as f32) / 2.0).round() as i32;
            let diff = (data_half[c] as i32 - expected_half).abs();
            assert!(
                diff <= 1,
                "channel {c} half-blend mismatch: got {}, expected {}",
                data_half[c],
                expected_half
            );
        }
    }

    #[test]
    fn a_raw_is_decoded_through_the_media_raw_ladder() {
        // Synthesise a minimal TIFF-based RAW file carrying an embedded JPEG preview
        let temp = tempfile::tempdir().unwrap();
        let raw_path = temp.path().join("test_shot.nef");

        let w = 64u32;
        let h = 48u32;
        let mut jpeg_bytes = Vec::new();
        let preview_img = image::RgbImage::from_pixel(w, h, image::Rgb([180, 120, 60]));
        let mut cursor = std::io::Cursor::new(&mut jpeg_bytes);
        preview_img
            .write_to(&mut cursor, image::ImageFormat::Jpeg)
            .unwrap();

        let mut tiff = Vec::new();
        tiff.extend_from_slice(b"II");
        tiff.extend_from_slice(&42u16.to_le_bytes());
        tiff.extend_from_slice(&8u32.to_le_bytes()); // IFD0 offset

        let num_entries = 4u16;
        let ifd_len = 2 + 12 * (num_entries as usize) + 4;
        let jpeg_offset = (8 + ifd_len) as u32;

        tiff.extend_from_slice(&num_entries.to_le_bytes());
        // Tag 0x0100 (ImageWidth), LONG (4), count 1, value w
        tiff.extend_from_slice(&0x0100u16.to_le_bytes());
        tiff.extend_from_slice(&4u16.to_le_bytes());
        tiff.extend_from_slice(&1u32.to_le_bytes());
        tiff.extend_from_slice(&w.to_le_bytes());

        // Tag 0x0101 (ImageLength), LONG (4), count 1, value h
        tiff.extend_from_slice(&0x0101u16.to_le_bytes());
        tiff.extend_from_slice(&4u16.to_le_bytes());
        tiff.extend_from_slice(&1u32.to_le_bytes());
        tiff.extend_from_slice(&h.to_le_bytes());

        // Tag 0x0201 (JPEGInterchangeFormat), LONG (4), count 1, offset jpeg_offset
        tiff.extend_from_slice(&0x0201u16.to_le_bytes());
        tiff.extend_from_slice(&4u16.to_le_bytes());
        tiff.extend_from_slice(&1u32.to_le_bytes());
        tiff.extend_from_slice(&jpeg_offset.to_le_bytes());

        // Tag 0x0202 (JPEGInterchangeFormatLength), LONG (4), count 1, length
        tiff.extend_from_slice(&0x0202u16.to_le_bytes());
        tiff.extend_from_slice(&4u16.to_le_bytes());
        tiff.extend_from_slice(&1u32.to_le_bytes());
        tiff.extend_from_slice(&(jpeg_bytes.len() as u32).to_le_bytes());

        // Next IFD = 0
        tiff.extend_from_slice(&0u32.to_le_bytes());
        tiff.extend_from_slice(&jpeg_bytes);

        std::fs::write(&raw_path, &tiff).unwrap();

        // Decode through decode_image, which delegates RAWs to media::raw
        let decoded =
            decode_image(&raw_path).expect("RAW decoding through media::raw ladder must succeed");
        assert_eq!(decoded.width(), w);
        assert_eq!(decoded.height(), h);
        match decoded {
            ImageBuffer::Rgb8 { data, .. } => {
                assert_eq!(data.len(), (w * h * 3) as usize);
            }
            ImageBuffer::Rgb16 { .. } => {
                panic!("RAW ladder preview decoded as Rgb16 instead of Rgb8")
            }
        }
    }

    #[test]
    fn a_16_bit_greyscale_tiff_keeps_all_16_bits() {
        let temp = tempfile::tempdir().unwrap();
        let tiff_path = temp.path().join("film_scan_16bit_gray.tiff");

        let w = 4u32;
        let h = 2u32;
        // Distinct values requiring more than 8 bits of tone
        let luma_values: Vec<u16> = vec![
            0x0102, // 258: 8-bit truncation loses upper byte
            0x1234, // 4660
            0x8000, // 32768
            0xFFFF, // 65535
            0x00FF, // 255: lower byte only
            0x0F00, // 3840: upper byte only
            0xABCD, // 43981
            0x5432, // 21554
        ];

        let gray_buf =
            image::ImageBuffer::<image::Luma<u16>, Vec<u16>>::from_raw(w, h, luma_values.clone())
                .unwrap();
        gray_buf.save(&tiff_path).unwrap();

        let decoded = decode_image(&tiff_path).expect("16-bit greyscale TIFF must decode");
        assert_eq!(decoded.width(), w);
        assert_eq!(decoded.height(), h);
        let ImageBuffer::Rgb16 { data, .. } = decoded else {
            panic!("expected ImageBuffer::Rgb16, got {decoded:?}");
        };
        assert_eq!(data.len(), (w * h * 3) as usize);
        for (i, &expected_luma) in luma_values.iter().enumerate() {
            assert_eq!(data[i * 3], expected_luma, "R mismatch at pixel {i}");
            assert_eq!(data[i * 3 + 1], expected_luma, "G mismatch at pixel {i}");
            assert_eq!(data[i * 3 + 2], expected_luma, "B mismatch at pixel {i}");
        }
    }

    #[test]
    fn whites_plus_one_boosts_upper_highlights_and_leaves_mid_grey_untouched() {
        for whites in [1.0f32, 100.0f32] {
            let mut lin = LinearBuffer {
                width: 2,
                height: 1,
                data: vec![0.18, 0.18, 0.18, 0.9, 0.9, 0.9],
            };
            let recipe = AdjustmentRecipe {
                whites,
                ..AdjustmentRecipe::default()
            };
            lin.apply_adjustments(&recipe);
            assert_eq!(lin.data[0], 0.18);
            assert_eq!(lin.data[1], 0.18);
            assert_eq!(lin.data[2], 0.18);
            assert!(lin.data[3] > 0.9);
            assert!(lin.data[4] > 0.9);
            assert!(lin.data[5] > 0.9);
        }
    }

    #[test]
    fn blacks_minus_one_crushes_deep_shadows_and_leaves_mid_grey_untouched() {
        for blacks in [-1.0f32, -100.0f32] {
            let mut lin = LinearBuffer {
                width: 2,
                height: 1,
                data: vec![0.18, 0.18, 0.18, 0.01, 0.01, 0.01],
            };
            let recipe = AdjustmentRecipe {
                blacks,
                ..AdjustmentRecipe::default()
            };
            lin.apply_adjustments(&recipe);
            assert_eq!(lin.data[0], 0.18);
            assert_eq!(lin.data[1], 0.18);
            assert_eq!(lin.data[2], 0.18);
            assert!(lin.data[3] < 0.01);
            assert!(lin.data[4] < 0.01);
            assert!(lin.data[5] < 0.01);
        }
    }

    #[test]
    fn brightness_shifts_mid_tones_without_changing_black_or_white() {
        for brightness in [-50.0f32, 50.0f32] {
            let mut lin = LinearBuffer {
                width: 3,
                height: 1,
                data: vec![0.0, 0.0, 0.0, 0.18, 0.18, 0.18, 1.0, 1.0, 1.0],
            };
            let recipe = AdjustmentRecipe {
                brightness,
                ..AdjustmentRecipe::default()
            };
            lin.apply_adjustments(&recipe);
            assert_eq!(lin.data[0], 0.0);
            assert_eq!(lin.data[1], 0.0);
            assert_eq!(lin.data[2], 0.0);
            assert!((lin.data[3] - 0.18).abs() > 0.01);
            assert!((lin.data[6] - 1.0).abs() < 1e-6);
            assert!((lin.data[7] - 1.0).abs() < 1e-6);
            assert!((lin.data[8] - 1.0).abs() < 1e-6);
        }
    }

    #[test]
    fn hue_rotation_preserves_luma_and_saturation() {
        use super::super::color::{linear_srgb_to_oklab, rotate_hue_oklch};
        // In Björn Ottosson's Oklab / OkLCh perceptual color space, rotating hue
        // angle at constant L and C preserves perceptual Lightness (L) and Chroma (C)
        // within floating-point tolerance.
        let colors = [(0.8f32, 0.2f32, 0.3f32), (0.2, 0.7, 0.4), (0.3, 0.4, 0.8)];
        for &(r, g, b) in &colors {
            let (orig_l, orig_a, orig_b) = linear_srgb_to_oklab(r, g, b);
            let orig_chroma = (orig_a * orig_a + orig_b * orig_b).sqrt();

            for delta in [30.0f32, 60.0, 90.0, 180.0, -45.0, -120.0] {
                let (rot_r, rot_g, rot_b) = rotate_hue_oklch(r, g, b, delta);
                let (rot_l, rot_a, rot_b) = linear_srgb_to_oklab(rot_r, rot_g, rot_b);
                let rot_chroma = (rot_a * rot_a + rot_b * rot_b).sqrt();

                assert!(
                    (orig_l - rot_l).abs() < 1e-5,
                    "Oklab L changed: orig {orig_l}, rot {rot_l} for delta {delta}"
                );
                assert!(
                    (orig_chroma - rot_chroma).abs() < 1e-5,
                    "Oklab C (saturation) changed: orig {orig_chroma}, rot {rot_chroma} for delta {delta}"
                );
            }
        }
    }
}
