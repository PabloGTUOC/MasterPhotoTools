//! F4 — half-frame film split.
//!
//! A half-frame camera exposes two images per 35 mm frame, so a scan contains
//! two photographs side by side. The reference format is the Pentax 17: a
//! 17 × 24 mm frame, always portrait, aspect ratio 24/17 ≈ 1.41.

use crate::error::Error;
use crate::jobs::{Outcome, Progress, ToolResult};
use crate::media::jpeg::JpegOptions;
use crate::media::{image_ops, slices};
use crate::tools::{expand_inputs, Plan, Tool};
use image::DynamicImage;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const ACCEPTED: [&str; 5] = ["jpg", "jpeg", "png", "tif", "tiff"];
pub const QUALITY: u8 = 95;

/// F4's defaults, exactly as the specification tabulates them.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SplitSettings {
    /// Pixel value at or below which a pixel counts as black.
    pub threshold_dark: u8,
    /// Pixel value at or above which a pixel counts as white.
    pub threshold_white: u8,
    /// Fraction of extreme pixels needed to call a line "border".
    pub border_tol: f32,
    /// Maximum proportion removable from one side.
    pub max_crop_pct: f32,
    /// Proportion of width ignored at each end when seeking the divider.
    pub margin: f32,
    /// Refinement radius around the darkest column, in pixels.
    pub window: usize,
    /// Target height ÷ width.
    pub ratio: f32,
}

impl Default for SplitSettings {
    fn default() -> Self {
        Self {
            threshold_dark: 25,
            threshold_white: 235,
            border_tol: 0.92,
            max_crop_pct: 0.12,
            margin: 0.20,
            window: 20,
            ratio: 24.0 / 17.0,
        }
    }
}

/// How far past the target ratio a half may be before the excess is trimmed.
const RATIO_SLACK: f32 = 0.10;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitParams {
    pub inputs: Vec<PathBuf>,
    pub recursive: bool,
    pub settings: SplitSettings,
    pub out_dir: PathBuf,
}

impl SplitParams {
    pub fn new(inputs: Vec<PathBuf>, out_dir: PathBuf) -> Self {
        Self {
            inputs,
            recursive: false,
            settings: SplitSettings::default(),
            out_dir,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitAction {
    pub source: PathBuf,
    /// `{base}_A.jpg`
    pub target_a: PathBuf,
    /// `{base}_B.jpg`
    pub target_b: PathBuf,
    pub settings: SplitSettings,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SplitSummary {
    pub written: Vec<PathBuf>,
    pub failures: Vec<(PathBuf, String)>,
}

/// What a preview returns: the border-cropped whole image plus both halves,
/// with nothing written to disk.
pub struct SplitPreview {
    pub cropped: DynamicImage,
    pub a: DynamicImage,
    pub b: DynamicImage,
    /// The divider column, in the border-cropped image's coordinates.
    pub divider_x: u32,
}

/// Run the split procedure without writing anything.
pub fn preview(source: &std::path::Path, settings: &SplitSettings) -> Result<SplitPreview, Error> {
    let img = image_ops::decode_oriented(source)?;

    // 1. Remove the lab border.
    let cropped = remove_border(&img, settings);

    // 2. Locate the divider.
    let divider_x = find_divider(&cropped, settings)?;

    // 3. Split.
    let (left, right) = split_at(&cropped, divider_x);

    // 4. Trim residual dark bands, respecting the frame ratio.
    let a = finish_half(&left, settings);
    let b = finish_half(&right, settings);

    Ok(SplitPreview {
        cropped,
        a,
        b,
        divider_x,
    })
}

/// Step 1 — scan inward from each edge for the lab's white or black surround.
fn remove_border(img: &DynamicImage, s: &SplitSettings) -> DynamicImage {
    let luma = img.to_luma8();
    let (w, h) = (luma.width() as usize, luma.height() as usize);

    let bounds = slices::scan_border_inward(
        luma.as_raw(),
        w,
        h,
        s.threshold_dark,
        s.threshold_white,
        s.border_tol,
        s.max_crop_pct,
    );

    if bounds.width() < 2 || bounds.height() < 2 {
        return img.clone();
    }
    img.crop_imm(bounds.left, bounds.top, bounds.width(), bounds.height())
}

/// Step 2 — the darkest column, ignoring `margin` at each end, refined within
/// `±window`.
fn find_divider(img: &DynamicImage, s: &SplitSettings) -> Result<u32, Error> {
    let luma = img.to_luma8();
    let (w, h) = (luma.width() as usize, luma.height() as usize);
    let profile = slices::column_mean_profile(luma.as_raw(), w, h);

    slices::darkest_column(&profile, s.margin, s.window)
        .map(|x| x as u32)
        .ok_or_else(|| {
            Error::Internal(
                "Could not locate a divider: the scan is too narrow for the configured margin"
                    .into(),
            )
        })
}

/// Step 3 — cut at the divider, dropping the divider column itself.
fn split_at(img: &DynamicImage, divider_x: u32) -> (DynamicImage, DynamicImage) {
    let w = img.width();
    let h = img.height();

    let left_w = divider_x.min(w);
    let right_x = (divider_x + 1).min(w);
    let right_w = w.saturating_sub(right_x);

    (
        img.crop_imm(0, 0, left_w.max(1), h),
        img.crop_imm(right_x, 0, right_w.max(1), h),
    )
}

/// Step 4 — trim residual dark bands, rotate a landscape half upright, and
/// remove any excess height beyond the frame ratio from the bottom only.
fn finish_half(half: &DynamicImage, s: &SplitSettings) -> DynamicImage {
    // Trim dark bands from all four sides, but never past the frame ratio.
    let luma = half.to_luma8();
    let (w, h) = (luma.width() as usize, luma.height() as usize);
    let bounds = slices::trim_dark_edges(
        luma.as_raw(),
        w,
        h,
        s.threshold_dark,
        s.border_tol,
        (w.min(h) as f32 * s.max_crop_pct) as usize,
        0,
    );

    let mut out = if bounds.width() >= 2 && bounds.height() >= 2 {
        half.crop_imm(bounds.left, bounds.top, bounds.width(), bounds.height())
    } else {
        half.clone()
    };

    // A half that came out landscape is rotated upright first.
    if out.width() > out.height() {
        out = out.rotate90();
    }

    // If the result is still more than 10% taller than the target ratio, remove
    // the excess from the bottom only.
    let target_h = (out.width() as f32 * s.ratio).round();
    if target_h > 0.0 && (out.height() as f32) > target_h * (1.0 + RATIO_SLACK) {
        let keep = target_h.round() as u32;
        if keep >= 1 && keep < out.height() {
            out = out.crop_imm(0, 0, out.width(), keep);
        }
    }

    out
}

pub struct SplitTool;

impl Tool for SplitTool {
    type Params = SplitParams;
    type Action = SplitAction;
    type Summary = SplitSummary;

    /// Dry run. Creates nothing — not even the output directory.
    fn plan(&self, p: &Self::Params) -> ToolResult<Plan<Self::Action>> {
        let (files, skipped) = expand_inputs(&p.inputs, p.recursive, &ACCEPTED);

        let actions = files
            .into_iter()
            .map(|source| {
                let stem = source.file_stem().unwrap_or_default().to_string_lossy();
                SplitAction {
                    target_a: p.out_dir.join(format!("{stem}_A.jpg")),
                    target_b: p.out_dir.join(format!("{stem}_B.jpg")),
                    source,
                    settings: p.settings,
                }
            })
            .collect();

        Ok(Outcome {
            data: Plan { actions, skipped },
        })
    }

    fn apply(
        &self,
        plan: Plan<Self::Action>,
        progress: &dyn Progress,
    ) -> ToolResult<Self::Summary> {
        let total = plan.actions.len() as u64;
        let mut summary = SplitSummary::default();

        for (done, action) in plan.actions.into_iter().enumerate() {
            if progress.cancelled() {
                break;
            }
            progress.report(done as u64, total, &action.source.to_string_lossy());

            match split_one(&action) {
                Ok(()) => {
                    summary.written.push(action.target_a);
                    summary.written.push(action.target_b);
                }
                Err(e) => summary.failures.push((action.source, e.to_string())),
            }
        }

        progress.report(total, total, "done");
        Ok(Outcome { data: summary })
    }
}

fn split_one(action: &SplitAction) -> Result<(), Error> {
    if let Some(parent) = action.target_a.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let result = preview(&action.source, &action.settings)?;

    // Step 5: quality 95 with no chroma subsampling.
    let options = JpegOptions::deliverable(QUALITY);
    image_ops::write_jpeg_with(&result.a, &action.target_a, &options)?;
    image_ops::write_jpeg_with(&result.b, &action.target_b, &options)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_match_the_specification_table() {
        let s = SplitSettings::default();
        assert_eq!(s.threshold_dark, 25);
        assert_eq!(s.threshold_white, 235);
        assert_eq!(s.border_tol, 0.92);
        assert_eq!(s.max_crop_pct, 0.12);
        assert_eq!(s.margin, 0.20);
        assert_eq!(s.window, 20);
        // 24/17 for the Pentax 17's 17 × 24 mm frame.
        assert!((s.ratio - 24.0 / 17.0).abs() < 1e-6);
        assert!((s.ratio - 1.41).abs() < 0.01);
    }

    #[test]
    fn the_ratio_is_configurable_for_other_cameras() {
        let s = SplitSettings {
            ratio: 3.0 / 2.0,
            ..SplitSettings::default()
        };
        assert!((s.ratio - 1.5).abs() < 1e-6);
    }

    #[test]
    fn splitting_drops_the_divider_column_itself() {
        let img = DynamicImage::ImageRgb8(image::RgbImage::new(101, 40));
        let (a, b) = split_at(&img, 50);
        assert_eq!(a.width(), 50, "left half stops before the divider");
        assert_eq!(b.width(), 50, "right half starts after it");
        assert_eq!(a.height(), 40);
    }

    #[test]
    fn splitting_at_an_edge_still_produces_two_images() {
        let img = DynamicImage::ImageRgb8(image::RgbImage::new(20, 10));
        let (a, b) = split_at(&img, 0);
        assert!(a.width() >= 1 && b.width() >= 1);
    }
}
