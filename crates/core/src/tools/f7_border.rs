//! F7 — print border.
//!
//! Places an image on a fixed white canvas with rounded corners, sized for
//! printing and for social platforms that crop unpredictably.

use crate::error::Error;
use crate::jobs::{Outcome, Progress, ToolResult};
use crate::media::jpeg::JpegOptions;
use crate::media::{image_ops, slices};
use crate::tools::{expand_inputs, Plan, Tool};
use image::{DynamicImage, ImageBuffer, Rgb, RgbImage, Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const ACCEPTED: [&str; 6] = ["jpg", "jpeg", "png", "tif", "tiff", "webp"];

/// The canvas is always this wide; the height follows from the orientation.
pub const CANVAS_WIDTH: u32 = 3000;
/// Portrait input yields 4:5 — 3000 × 3750.
pub const PORTRAIT_HEIGHT: u32 = 3750;
/// Landscape input yields 5:4 — 3000 × 2400.
pub const LANDSCAPE_HEIGHT: u32 = 2400;

/// Minimum white margin on every side.
pub const MIN_MARGIN: u32 = 50;
/// Corner radius as a proportion of the image's short side.
pub const CORNER_RADIUS_FRACTION: f32 = 0.02;
/// The corner mask is rendered at this multiple and downsampled, for antialiasing.
pub const MASK_SUPERSAMPLE: u32 = 4;

/// Dark-edge trim (step 1).
pub const TRIM_LUMA: u8 = 28;
pub const TRIM_TOLERANCE: f32 = 0.70;
pub const TRIM_MAX_PX: usize = 40;
pub const TRIM_INSET: u32 = 1;

pub const QUALITY: u8 = 95;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrintBorderParams {
    pub inputs: Vec<PathBuf>,
    pub recursive: bool,
    /// Trim dark scan edges before placing the image.
    pub trim_dark_edges: bool,
    /// How the canvas looks. Defaults to §F7's fixed appearance.
    pub style: BorderStyle,
    pub out_dir: PathBuf,
}

impl PrintBorderParams {
    pub fn new(inputs: Vec<PathBuf>, out_dir: PathBuf) -> Self {
        Self {
            inputs,
            recursive: false,
            trim_dark_edges: true,
            style: BorderStyle::default(),
            out_dir,
        }
    }
}

/// The canvas for an input of a given shape.
///
/// The canvas for an input of this shape, at this width.
///
/// This encodes the *rule* rather than taking both dimensions from the caller:
/// no single pair of edge lengths can produce both 4:5 and 5:4. At the
/// specification's 3000 px the results are its 3000×3750 and 3000×2400.
pub fn canvas_for_width(width: u32, height: u32, canvas_width: u32) -> (u32, u32) {
    let w = canvas_width.max(1);
    if height > width {
        // 4:5 — taller than wide.
        (w, (w as u64 * 5 / 4) as u32)
    } else {
        // 5:4 — wider than tall.
        (w, (w as u64 * 4 / 5) as u32)
    }
}

/// The specification's canvas, at its fixed 3000 px width.
pub fn canvas_for(width: u32, height: u32) -> (u32, u32) {
    canvas_for_width(width, height, CANVAS_WIDTH)
}

/// How the canvas looks.
///
/// §F7 fixes all of this — a white canvas, a 50 px margin, a 2% radius, 3000 px
/// wide — and the defaults here are those values, so an untouched run produces
/// exactly what the specification describes. They are parameters because a
/// person asked to choose them; the consistency the fixed version guaranteed is
/// now theirs to keep or spend.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BorderStyle {
    /// The canvas the photograph sits on.
    pub canvas_colour: [u8; 3],
    /// Canvas width. The height follows from the input's shape.
    pub canvas_width: u32,
    /// Smallest gap between the photograph and any edge.
    pub min_margin: u32,
    /// Corner radius as a proportion of the placed image's short side.
    pub corner_radius_fraction: f32,
}

impl Default for BorderStyle {
    fn default() -> Self {
        Self {
            canvas_colour: [255, 255, 255],
            canvas_width: CANVAS_WIDTH,
            min_margin: MIN_MARGIN,
            corner_radius_fraction: CORNER_RADIUS_FRACTION,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrintBorderAction {
    pub source: PathBuf,
    pub target: PathBuf,
    pub trim_dark_edges: bool,
    pub style: BorderStyle,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PrintBorderSummary {
    pub written: Vec<PathBuf>,
    pub failures: Vec<(PathBuf, String)>,
}

pub struct PrintBorderTool;

impl Tool for PrintBorderTool {
    type Params = PrintBorderParams;
    type Action = PrintBorderAction;
    type Summary = PrintBorderSummary;

    /// Dry run. Creates nothing.
    fn plan(&self, p: &Self::Params) -> ToolResult<Plan<Self::Action>> {
        let (files, skipped) = expand_inputs(&p.inputs, p.recursive, &ACCEPTED);

        let actions = files
            .into_iter()
            .map(|source| {
                let stem = source.file_stem().unwrap_or_default().to_string_lossy();
                PrintBorderAction {
                    target: p.out_dir.join(format!("{stem}.jpg")),
                    source,
                    trim_dark_edges: p.trim_dark_edges,
                    style: p.style,
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
        let mut summary = PrintBorderSummary::default();

        for (done, action) in plan.actions.into_iter().enumerate() {
            if progress.cancelled() {
                break;
            }
            progress.report(done as u64, total, &action.source.to_string_lossy());

            match border_one(&action) {
                Ok(()) => summary.written.push(action.target),
                Err(e) => summary.failures.push((action.source, e.to_string())),
            }
        }

        progress.report(total, total, "done");
        Ok(Outcome { data: summary })
    }
}

fn border_one(action: &PrintBorderAction) -> Result<(), Error> {
    if let Some(parent) = action.target.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut img = image_ops::decode_oriented(&action.source)?;

    // 1. Optionally trim dark scan edges.
    if action.trim_dark_edges {
        img = trim_edges(&img);
    }

    // 2. Choose the canvas from the input's shape.
    let style = action.style;
    let (canvas_w, canvas_h) = canvas_for_width(img.width(), img.height(), style.canvas_width);

    // 3. Fit inside the minimum margin, enlarging a smaller image to fill it.
    let available_w = canvas_w.saturating_sub(2 * style.min_margin).max(1);
    let available_h = canvas_h.saturating_sub(2 * style.min_margin).max(1);
    let scale =
        (available_w as f64 / img.width() as f64).min(available_h as f64 / img.height() as f64);
    let placed_w = ((img.width() as f64 * scale).floor() as u32).max(1);
    let placed_h = ((img.height() as f64 * scale).floor() as u32).max(1);
    let placed = image_ops::resize(&img, placed_w, placed_h)?;

    // 4. Round the corners, antialiased by a supersampled mask.
    let radius = (placed_w.min(placed_h) as f32 * style.corner_radius_fraction).round() as u32;
    let masked = rounded_corners(&placed.to_rgba8(), radius);

    // 5. Centre on the canvas.
    let mut canvas: RgbImage =
        ImageBuffer::from_pixel(canvas_w, canvas_h, Rgb(style.canvas_colour));
    let x0 = (canvas_w - placed_w) / 2;
    let y0 = (canvas_h - placed_h) / 2;

    for (x, y, pixel) in masked.enumerate_pixels() {
        let alpha = pixel[3] as f32 / 255.0;
        if alpha <= 0.0 {
            continue;
        }
        let target = canvas.get_pixel_mut(x0 + x, y0 + y);
        for c in 0..3 {
            // Blended against the canvas, not against white: a dark canvas
            // would otherwise show a pale fringe around every rounded corner,
            // which is the one place this is visible.
            target[c] = ((1.0 - alpha) * style.canvas_colour[c] as f32 + alpha * pixel[c] as f32)
                .round()
                .clamp(0.0, 255.0) as u8;
        }
    }

    // Save at quality 95 with no chroma subsampling.
    image_ops::write_jpeg_with(
        &DynamicImage::ImageRgb8(canvas),
        &action.target,
        &JpegOptions::deliverable(QUALITY),
    )
}

/// Step 1: trim while more than 70% of a sampled band falls below luma 28, up to
/// 40 px per side, plus a 1 px safety inset.
fn trim_edges(img: &DynamicImage) -> DynamicImage {
    let luma = img.to_luma8();
    let (w, h) = (luma.width() as usize, luma.height() as usize);

    let bounds = slices::trim_dark_edges(
        luma.as_raw(),
        w,
        h,
        TRIM_LUMA,
        TRIM_TOLERANCE,
        TRIM_MAX_PX,
        TRIM_INSET,
    );

    if bounds.width() == 0 || bounds.height() == 0 {
        return img.clone();
    }
    img.crop_imm(bounds.left, bounds.top, bounds.width(), bounds.height())
}

/// Step 4: rounded corners with an antialiased edge.
///
/// The mask is evaluated at `MASK_SUPERSAMPLE`× and averaged down, so a corner
/// is a smooth ramp rather than a staircase.
pub fn rounded_corners(img: &RgbaImage, radius: u32) -> RgbaImage {
    let mut out = img.clone();
    if radius == 0 {
        return out;
    }

    let (w, h) = (img.width(), img.height());
    let s = MASK_SUPERSAMPLE;
    let r = radius as f32;
    let samples = (s * s) as f32;

    for y in 0..h {
        for x in 0..w {
            // Only the four corner squares can be affected.
            let near_left = x < radius;
            let near_right = x + radius >= w;
            let near_top = y < radius;
            let near_bottom = y + radius >= h;
            if !((near_left || near_right) && (near_top || near_bottom)) {
                continue;
            }

            // Centre of the corner arc this pixel belongs to.
            let cx = if near_left { r } else { w as f32 - r };
            let cy = if near_top { r } else { h as f32 - r };

            let mut inside = 0u32;
            for sy in 0..s {
                for sx in 0..s {
                    let px = x as f32 + (sx as f32 + 0.5) / s as f32;
                    let py = y as f32 + (sy as f32 + 0.5) / s as f32;
                    let dx = px - cx;
                    let dy = py - cy;
                    // Only the quadrant outside the arc centre is curved.
                    let outside_x = (near_left && dx < 0.0) || (near_right && dx > 0.0);
                    let outside_y = (near_top && dy < 0.0) || (near_bottom && dy > 0.0);
                    if !(outside_x && outside_y) || dx * dx + dy * dy <= r * r {
                        inside += 1;
                    }
                }
            }

            let coverage = inside as f32 / samples;
            let pixel = out.get_pixel_mut(x, y);
            pixel[3] = (pixel[3] as f32 * coverage).round().clamp(0.0, 255.0) as u8;
        }
    }

    out
}

/// Expose the empty-mask case for tests and callers.
pub fn transparent() -> Rgba<u8> {
    Rgba([0, 0, 0, 0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_portrait_input_yields_a_four_by_five_canvas() {
        assert_eq!(canvas_for(2000, 3000), (3000, 3750));
        // 4:5 exactly.
        assert_eq!(3000 * 5, 3750 * 4);
    }

    #[test]
    fn a_landscape_input_yields_a_five_by_four_canvas() {
        assert_eq!(canvas_for(3000, 2000), (3000, 2400));
        // 5:4 exactly.
        assert_eq!(3000 * 4, 2400 * 5);
    }

    #[test]
    fn a_square_input_is_treated_as_landscape() {
        assert_eq!(canvas_for(1000, 1000), (3000, 2400));
    }

    #[test]
    fn both_canvases_share_the_same_width() {
        assert_eq!(canvas_for(10, 20).0, canvas_for(20, 10).0);
        assert_eq!(canvas_for(10, 20).0, CANVAS_WIDTH);
    }

    #[test]
    fn a_zero_radius_leaves_the_image_untouched() {
        let img: RgbaImage = ImageBuffer::from_pixel(10, 10, Rgba([1, 2, 3, 255]));
        let out = rounded_corners(&img, 0);
        assert_eq!(out, img);
    }

    #[test]
    fn rounded_corners_clear_the_corner_and_keep_the_centre() {
        let size = 40;
        let img: RgbaImage = ImageBuffer::from_pixel(size, size, Rgba([10, 20, 30, 255]));
        let out = rounded_corners(&img, 10);

        // The extreme corner pixel is outside the arc entirely.
        assert_eq!(out.get_pixel(0, 0)[3], 0, "corner should be cut away");
        assert_eq!(out.get_pixel(size - 1, size - 1)[3], 0);

        // The centre is untouched.
        assert_eq!(out.get_pixel(size / 2, size / 2)[3], 255);
        // An edge midpoint is not a corner and stays opaque.
        assert_eq!(out.get_pixel(size / 2, 0)[3], 255);
    }

    #[test]
    fn the_corner_edge_is_antialiased_rather_than_a_hard_step() {
        let size = 60;
        let img: RgbaImage = ImageBuffer::from_pixel(size, size, Rgba([10, 20, 30, 255]));
        let out = rounded_corners(&img, 20);

        // Somewhere along the arc there must be partial coverage; a hard mask
        // would only ever produce 0 or 255.
        let partial = (0..20)
            .flat_map(|y| (0..20).map(move |x| (x, y)))
            .any(|(x, y)| {
                let a = out.get_pixel(x, y)[3];
                a > 0 && a < 255
            });
        assert!(
            partial,
            "supersampled mask should produce intermediate alpha"
        );
    }

    #[test]
    fn the_trim_constants_match_the_specification() {
        assert_eq!(TRIM_LUMA, 28);
        assert_eq!(TRIM_TOLERANCE, 0.70);
        assert_eq!(TRIM_MAX_PX, 40);
        assert_eq!(TRIM_INSET, 1);
        assert_eq!(MIN_MARGIN, 50);
        assert_eq!(MASK_SUPERSAMPLE, 4);
    }

    /// The defaults are the specification's canvas, so an untouched run still
    /// produces exactly what §F7 describes.
    #[test]
    fn the_default_style_is_the_specifications_fixed_canvas() {
        let style = BorderStyle::default();
        assert_eq!(style.canvas_colour, [255, 255, 255]);
        assert_eq!(style.canvas_width, CANVAS_WIDTH);
        assert_eq!(style.min_margin, MIN_MARGIN);
        assert_eq!(style.corner_radius_fraction, CORNER_RADIUS_FRACTION);

        assert_eq!(canvas_for(2000, 3000), (CANVAS_WIDTH, PORTRAIT_HEIGHT));
        assert_eq!(canvas_for(3000, 2000), (CANVAS_WIDTH, LANDSCAPE_HEIGHT));
    }

    /// A different width keeps the two aspect ratios.
    #[test]
    fn a_narrower_canvas_keeps_the_four_five_and_five_four_shapes() {
        assert_eq!(canvas_for_width(2000, 3000, 1000), (1000, 1250));
        assert_eq!(canvas_for_width(3000, 2000, 1000), (1000, 800));
    }

    /// The corners are blended against the canvas, not against white.
    ///
    /// Blending a rounded corner against white on a black canvas leaves a pale
    /// fringe around every photograph — the one place the hardcoded colour was
    /// visible, and the thing a careless version of this change gets wrong.
    #[test]
    fn a_dark_canvas_does_not_fringe_the_rounded_corners() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("frame.jpg");
        let out = dir.path().join("out");

        // A mid-grey photograph, so any pale pixel is the canvas leaking.
        let img = image::RgbImage::from_pixel(400, 600, image::Rgb([128, 128, 128]));
        image::DynamicImage::ImageRgb8(img).save(&source).unwrap();

        let mut params = PrintBorderParams::new(vec![source], out.clone());
        params.style = BorderStyle {
            canvas_colour: [0, 0, 0],
            canvas_width: 600,
            min_margin: 20,
            corner_radius_fraction: 0.2,
        };

        let plan = PrintBorderTool.plan(&params).unwrap().data;
        PrintBorderTool
            .apply(plan, &crate::jobs::InMemoryProgress::default())
            .unwrap();

        let written = image::open(out.join("frame.jpg")).unwrap().to_rgb8();

        // On a black canvas holding a mid-grey photograph, a bright pixel can
        // only be the canvas colour leaking through the corner blend.
        //
        // The bound is well above the grey rather than just above it: JPEG at
        // quality 95 rings around the hard corner edge and reaches 141 against
        // a 128 flat, which is compression, not fringing. Blending against
        // white — the bug this guards — puts the corner pixels near 255, so
        // this still fails decisively if it comes back.
        let brightest = written
            .pixels()
            .map(|p| p[0].max(p[1]).max(p[2]))
            .max()
            .unwrap();
        assert!(
            brightest < 180,
            "the canvas is leaking into the corners: brightest pixel {brightest}"
        );
    }
}
