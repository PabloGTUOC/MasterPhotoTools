//! F6 — general-purpose transform over a file or a whole directory.

use crate::error::Error;
use crate::jobs::{Outcome, Progress, ToolResult};
use crate::media::image_ops;
use crate::tools::{expand_inputs, Plan, Skip, Tool};
use image::ImageFormat;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Extensions F6 accepts.
pub const ACCEPTED: [&str; 7] = ["jpg", "jpeg", "png", "tif", "tiff", "heic", "heif"];

/// Extensions the decoder cannot actually open, despite being accepted.
///
/// The `image` crate has no HEIC decoder. Reporting these as skipped is the
/// honest outcome; silently producing nothing is not.
const UNDECODABLE: [&str; 2] = ["heic", "heif"];

pub const DEFAULT_QUALITY: u8 = 95;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformParams {
    /// Files, directories, or a mix. Directories contribute their acceptable files.
    pub inputs: Vec<PathBuf>,
    pub recursive: bool,
    /// Degrees clockwise; the canvas expands to fit.
    pub rotate_degrees: Option<f32>,
    /// Cap on the long edge. Downscale only.
    pub max_long_edge: Option<u32>,
    pub format: Option<TargetFormat>,
    pub quality: u8,
    /// Optimise the encode where the format supports it.
    pub optimise: bool,
    pub out_dir: PathBuf,
}

impl TransformParams {
    pub fn new(inputs: Vec<PathBuf>, out_dir: PathBuf) -> Self {
        Self {
            inputs,
            recursive: false,
            rotate_degrees: None,
            max_long_edge: None,
            format: None,
            quality: DEFAULT_QUALITY,
            optimise: true,
            out_dir,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TargetFormat {
    Jpeg,
    Png,
    Tiff,
    WebP,
}

impl TargetFormat {
    pub fn extension(&self) -> &'static str {
        match self {
            TargetFormat::Jpeg => "jpg",
            TargetFormat::Png => "png",
            TargetFormat::Tiff => "tif",
            TargetFormat::WebP => "webp",
        }
    }

    fn image_format(&self) -> ImageFormat {
        match self {
            TargetFormat::Jpeg => ImageFormat::Jpeg,
            TargetFormat::Png => ImageFormat::Png,
            TargetFormat::Tiff => ImageFormat::Tiff,
            TargetFormat::WebP => ImageFormat::WebP,
        }
    }

    /// True where the target has no alpha channel, so it must be flattened.
    fn is_opaque(&self) -> bool {
        matches!(self, TargetFormat::Jpeg)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformAction {
    pub source: PathBuf,
    pub target: PathBuf,
    pub rotate_degrees: Option<f32>,
    pub max_long_edge: Option<u32>,
    pub format: TargetFormat,
    pub quality: u8,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TransformSummary {
    pub written: Vec<PathBuf>,
    pub failures: Vec<(PathBuf, String)>,
}

pub struct TransformTool;

impl Tool for TransformTool {
    type Params = TransformParams;
    type Action = TransformAction;
    type Summary = TransformSummary;

    /// Dry run. Creates nothing — not even the output directory.
    fn plan(&self, p: &Self::Params) -> ToolResult<Plan<Self::Action>> {
        let (files, mut skipped) = expand_inputs(&p.inputs, p.recursive, &ACCEPTED);
        let mut actions = Vec::new();

        for source in files {
            let extension = source
                .extension()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase();

            if UNDECODABLE.contains(&extension.as_str()) {
                skipped.push(Skip {
                    file: source.to_string_lossy().to_string(),
                    reason: format!("No decoder available for .{extension} files"),
                });
                continue;
            }

            // Keeping the source format means keeping its extension.
            let format = p.format.unwrap_or(match extension.as_str() {
                "png" => TargetFormat::Png,
                "tif" | "tiff" => TargetFormat::Tiff,
                _ => TargetFormat::Jpeg,
            });

            let stem = source.file_stem().unwrap_or_default().to_string_lossy();
            let target = p.out_dir.join(format!("{stem}.{}", format.extension()));

            if target == source {
                skipped.push(Skip {
                    file: source.to_string_lossy().to_string(),
                    reason: "Output would overwrite the input".into(),
                });
                continue;
            }

            actions.push(TransformAction {
                source,
                target,
                rotate_degrees: p.rotate_degrees,
                max_long_edge: p.max_long_edge,
                format,
                quality: p.quality,
            });
        }

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
        let mut summary = TransformSummary::default();

        for (done, action) in plan.actions.into_iter().enumerate() {
            if progress.cancelled() {
                break;
            }
            progress.report(done as u64, total, &action.source.to_string_lossy());

            match transform_one(&action) {
                Ok(()) => summary.written.push(action.target),
                Err(e) => summary.failures.push((action.source, e.to_string())),
            }
        }

        progress.report(total, total, "done");
        Ok(Outcome { data: summary })
    }
}

fn transform_one(action: &TransformAction) -> Result<(), Error> {
    if let Some(parent) = action.target.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // EXIF orientation is applied first, before any other geometry.
    let mut img = image_ops::decode_oriented(&action.source)?;

    if let Some(degrees) = action.rotate_degrees {
        img = image_ops::rotate_expanding(&img, degrees);
    }

    if let Some(max_edge) = action.max_long_edge {
        img = image_ops::downscale_to_max_edge(&img, max_edge)?;
    }

    if action.format.is_opaque() {
        img = image_ops::flatten_onto(&img, [255, 255, 255]);
    }

    image_ops::encode_to(
        &img,
        &action.target,
        action.format.image_format(),
        action.quality,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_target_format_maps_to_its_extension() {
        assert_eq!(TargetFormat::Jpeg.extension(), "jpg");
        assert_eq!(TargetFormat::Png.extension(), "png");
        assert_eq!(TargetFormat::Tiff.extension(), "tif");
        assert_eq!(TargetFormat::WebP.extension(), "webp");
    }

    #[test]
    fn only_jpeg_needs_flattening() {
        assert!(TargetFormat::Jpeg.is_opaque());
        assert!(!TargetFormat::Png.is_opaque());
        assert!(!TargetFormat::WebP.is_opaque());
    }

    #[test]
    fn the_default_quality_matches_the_specification() {
        assert_eq!(DEFAULT_QUALITY, 95);
        let p = TransformParams::new(vec![], PathBuf::from("/tmp"));
        assert_eq!(p.quality, 95);
        assert!(p.optimise, "optimise defaults to on");
    }
}
