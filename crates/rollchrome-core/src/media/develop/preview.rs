//! Develop preview generation and fast look re-rendering (RP-1, RP-12, DV-4).

use fast_image_resize as fr;
use phototools_core::error::Error;
use rayon::prelude::*;
use std::path::Path;

use crate::media::develop::render::{render, Look, OutputFormat, Rendered};
use crate::media::develop::{develop_linear, linear_to_developed, DevelopOptions, LinearProPhoto};
use crate::media::film::{render_film, FilmBackend, FilmOutput, Grain};
use crate::media::segment::to_upright;
use crate::tools::develop::ResolvedLook;

pub const PREVIEW_MAX_EDGE: u32 = 1600;
pub const PREVIEW_DRAFT_EDGE: u32 = 800;

/// Look passed to preview rendering: either a 3D LUT look or a film stock (DV-4).
#[derive(Debug, Clone, Copy)]
pub enum PreviewLook<'a> {
    Look(&'a Look),
    Film(&'a str),
}

impl<'a> From<&'a Look> for PreviewLook<'a> {
    fn from(l: &'a Look) -> Self {
        PreviewLook::Look(l)
    }
}

impl<'a> From<&'a str> for PreviewLook<'a> {
    fn from(s: &'a str) -> Self {
        PreviewLook::Film(s)
    }
}

impl<'a> From<&'a ResolvedLook> for PreviewLook<'a> {
    fn from(r: &'a ResolvedLook) -> Self {
        match r {
            ResolvedLook::Look(l) => PreviewLook::Look(l),
            ResolvedLook::Film { stock } => PreviewLook::Film(stock.as_str()),
        }
    }
}

/// Reduced-resolution developed RAW representation for interactive previewing (RP-1, RP-12).
///
/// Keeps the linear ProPhoto image developed with exposure 0, upright, reduced to `PREVIEW_MAX_EDGE`,
/// and a second copy reduced to `PREVIEW_DRAFT_EDGE = 800`.
/// Changing the look or exposure re-renders from this cached buffer without re-demosaicing
/// or re-reading the RAW from disk.
pub struct DevelopPreview {
    full: LinearProPhoto,
    draft: LinearProPhoto,
}

impl DevelopPreview {
    /// Develops raw file bytes to a reduced upright linear preview image (RP-1, RP-12).
    pub fn new(raw: &[u8], max_edge: u32) -> Result<Self, Error> {
        let linear = develop_linear(raw, &DevelopOptions { exposure_ev: 0.0 })?;
        Self::from_linear(linear, max_edge)
    }

    /// Creates a preview holding full and draft upright linear representations.
    pub fn from_linear(linear: LinearProPhoto, max_edge: u32) -> Result<Self, Error> {
        let (up_w, up_h, upright_pixels) = if linear.orientation <= 1 {
            (linear.width, linear.height, linear.pixels)
        } else {
            let mut flat = Vec::with_capacity(linear.pixels.len() * 3);
            for p in &linear.pixels {
                flat.push(p[0]);
                flat.push(p[1]);
                flat.push(p[2]);
            }
            let img = image::ImageBuffer::<image::Rgb<f32>, Vec<f32>>::from_raw(
                linear.width,
                linear.height,
                flat,
            )
            .ok_or_else(|| {
                Error::Internal("failed to create ImageBuffer from linear pixels".into())
            })?;
            let upright = to_upright(&img, linear.orientation as u32);
            let up_w = upright.width();
            let up_h = upright.height();
            let raw = upright.into_raw();
            let mut pixels = Vec::with_capacity((up_w * up_h) as usize);
            for chunk in raw.as_chunks::<3>().0 {
                pixels.push([chunk[0], chunk[1], chunk[2]]);
            }
            (up_w, up_h, pixels)
        };

        let full_edge = if max_edge > 0 {
            max_edge
        } else {
            PREVIEW_MAX_EDGE
        };
        let (full_w, full_h, full_pixels) = resize_linear(up_w, up_h, &upright_pixels, full_edge)?;

        let draft_edge = if full_edge < PREVIEW_DRAFT_EDGE {
            (full_edge / 2).max(1)
        } else {
            PREVIEW_DRAFT_EDGE
        };
        let (draft_w, draft_h, draft_pixels) =
            resize_linear(full_w, full_h, &full_pixels, draft_edge)?;

        Ok(Self {
            full: LinearProPhoto {
                width: full_w,
                height: full_h,
                pixels: full_pixels,
                orientation: 1,
                demosaic: linear.demosaic,
            },
            draft: LinearProPhoto {
                width: draft_w,
                height: draft_h,
                pixels: draft_pixels,
                orientation: 1,
                demosaic: linear.demosaic,
            },
        })
    }

    /// Upright, JPEG quality 90, with the look if any, at the given exposure and grain (RP-12, DV-4).
    /// Does not develop the RAW again.
    pub fn render<'a>(
        &self,
        look: Option<PreviewLook<'a>>,
        exposure_ev: f32,
        grain: Grain,
        draft: bool,
    ) -> Result<Rendered, Error> {
        self.render_with_data_dir(look, exposure_ev, grain, draft, None)
    }

    /// Upright preview rendering with optional custom film data directory.
    pub fn render_with_data_dir<'a>(
        &self,
        look: Option<PreviewLook<'a>>,
        exposure_ev: f32,
        grain: Grain,
        draft: bool,
        data_dir: Option<&Path>,
    ) -> Result<Rendered, Error> {
        if !exposure_ev.is_finite() || exposure_ev < -4.0 || exposure_ev > 4.0 {
            return Err(Error::Refused(format!(
                "exposure_ev {exposure_ev} is outside supported range -4.0..=4.0 or not finite"
            )));
        }

        let source = if draft { &self.draft } else { &self.full };
        let preview_look = look;

        if let Some(PreviewLook::Film(stock)) = preview_look {
            // No silent default folder for the film data (G10): the caller names it.
            let effective_dir = data_dir.ok_or_else(|| {
                Error::Refused("no folder for the film simulation data was given".to_string())
            })?;

            let rendered_pixels = render_film(
                &source.pixels,
                source.width,
                source.height,
                stock,
                grain,
                exposure_ev,
                FilmOutput::Srgb,
                FilmBackend::Auto,
                effective_dir,
            )?;

            let u8_data: Vec<u8> = rendered_pixels
                .into_par_iter()
                .flat_map(|p| {
                    [
                        (p[0] * 255.0).round().clamp(0.0, 255.0) as u8,
                        (p[1] * 255.0).round().clamp(0.0, 255.0) as u8,
                        (p[2] * 255.0).round().clamp(0.0, 255.0) as u8,
                    ]
                })
                .collect();
            let srgb_img = image::RgbImage::from_raw(source.width, source.height, u8_data)
                .ok_or_else(|| Error::Internal("failed to build preview image buffer".into()))?;
            let dyn_img = image::DynamicImage::ImageRgb8(srgb_img);
            let opts = phototools_core::media::jpeg::JpegOptions::deliverable(90);
            let bytes = phototools_core::media::jpeg::encode(&dyn_img, &opts)?;
            return Ok(Rendered {
                bytes,
                width: source.width,
                height: source.height,
                extension: "jpg",
            });
        }

        let look_obj = match preview_look {
            Some(PreviewLook::Look(l)) => Some(l),
            _ => None,
        };

        let exp_scale = 2.0f32.powf(exposure_ev);

        let scaled_pixels: Vec<[f32; 3]> = if (exp_scale - 1.0).abs() <= 1e-6 {
            source.pixels.clone()
        } else {
            source
                .pixels
                .par_iter()
                .map(|p| [p[0] * exp_scale, p[1] * exp_scale, p[2] * exp_scale])
                .collect()
        };

        let developed = linear_to_developed(
            source.width,
            source.height,
            &scaled_pixels,
            1,
            source.demosaic,
        )?;

        render(&developed, look_obj, OutputFormat::Jpeg { quality: 90 })
    }

    pub fn width(&self) -> u32 {
        self.full.width
    }

    pub fn height(&self) -> u32 {
        self.full.height
    }

    pub fn draft_width(&self) -> u32 {
        self.draft.width
    }

    pub fn draft_height(&self) -> u32 {
        self.draft.height
    }
}

fn resize_linear(
    w: u32,
    h: u32,
    pixels: &[[f32; 3]],
    max_edge: u32,
) -> Result<(u32, u32, Vec<[f32; 3]>), Error> {
    let long = w.max(h);
    if max_edge == 0 || long <= max_edge {
        return Ok((w, h, pixels.to_vec()));
    }

    let ratio = max_edge as f64 / long as f64;
    let new_w = ((w as f64 * ratio).floor() as u32).max(1);
    let new_h = ((h as f64 * ratio).floor() as u32).max(1);

    let byte_slice =
        unsafe { std::slice::from_raw_parts(pixels.as_ptr() as *const u8, pixels.len() * 12) };
    let src = fr::images::ImageRef::new(w, h, byte_slice, fr::PixelType::F32x3)
        .map_err(|e| Error::Internal(format!("resize linear preview source: {e:?}")))?;
    let mut dst = fr::images::Image::new(new_w, new_h, fr::PixelType::F32x3);
    fr::Resizer::new()
        .resize(&src, &mut dst, None)
        .map_err(|e| Error::Internal(format!("resize linear preview: {e:?}")))?;

    let dst_bytes = dst.into_vec();
    let mut out = Vec::with_capacity((new_w * new_h) as usize);
    for chunk in dst_bytes.as_chunks::<12>().0 {
        let r = f32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        let g = f32::from_ne_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
        let b = f32::from_ne_bytes([chunk[8], chunk[9], chunk[10], chunk[11]]);
        out.push([r, g, b]);
    }
    Ok((new_w, new_h, out))
}
