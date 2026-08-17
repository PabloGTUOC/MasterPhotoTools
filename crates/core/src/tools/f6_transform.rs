use crate::error::Error;
use crate::jobs::{Outcome, Progress, ToolResult};
use crate::media::image_ops::{decode, resize};
use crate::media::meta::ExifWriter;
use crate::tools::{Plan, Skip, Tool};
use image::{DynamicImage, ImageFormat};
use std::fs::File;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct TransformParams {
    pub paths: Vec<PathBuf>,
    pub rotate_deg: Option<f32>,
    pub max_long_edge: Option<u32>,
    pub format: Option<ImageFormat>,
    pub quality: u8,
    pub out_dir: PathBuf,
}

pub struct TransformAction {
    pub source: PathBuf,
    pub target: PathBuf,
    pub rotate_deg: Option<f32>,
    pub max_long_edge: Option<u32>,
    pub format: ImageFormat,
    pub quality: u8,
}

pub struct TransformTool;

impl Tool for TransformTool {
    type Params = TransformParams;
    type Action = TransformAction;
    type Summary = ();

    fn plan(&self, p: &Self::Params) -> ToolResult<Plan<Self::Action>> {
        let mut actions = Vec::new();
        let mut skipped = Vec::new();

        if !p.out_dir.exists() {
            std::fs::create_dir_all(&p.out_dir).map_err(Error::Io)?;
        }

        for path in &p.paths {
            if !path.exists() {
                skipped.push(Skip {
                    file: path.to_string_lossy().into(),
                    reason: "File not found".into(),
                });
                continue;
            }

            let ext = match p.format {
                Some(ImageFormat::Jpeg) => "jpg",
                Some(ImageFormat::Png) => "png",
                Some(ImageFormat::WebP) => "webp",
                _ => path
                    .extension()
                    .unwrap_or_default()
                    .to_str()
                    .unwrap_or("jpg"),
            };

            let name = path.file_stem().unwrap_or_default().to_string_lossy();
            let target = p.out_dir.join(format!("{}.{}", name, ext));

            actions.push(TransformAction {
                source: path.clone(),
                target,
                rotate_deg: p.rotate_deg,
                max_long_edge: p.max_long_edge,
                format: p.format.unwrap_or(ImageFormat::Jpeg),
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
        _progress: &dyn Progress,
    ) -> ToolResult<Self::Summary> {
        let mut writer = ExifWriter::start()?;

        for action in plan.actions {
            let mut img = decode(&action.source)?;

            if let Some(deg) = action.rotate_deg {
                if (deg - 90.0).abs() < 0.1 {
                    img = img.rotate90();
                } else if (deg - 180.0).abs() < 0.1 {
                    img = img.rotate180();
                } else if (deg - 270.0).abs() < 0.1 {
                    img = img.rotate270();
                }
            }

            if let Some(max_edge) = action.max_long_edge {
                let w = img.width();
                let h = img.height();
                let long = w.max(h);
                if long > max_edge {
                    let ratio = max_edge as f64 / long as f64;
                    let nw = (w as f64 * ratio).round() as u32;
                    let nh = (h as f64 * ratio).round() as u32;
                    img = resize(&img, nw, nh)?;
                }
            }

            if action.format == ImageFormat::Jpeg {
                img = DynamicImage::ImageRgb8(img.into_rgb8());
                let mut file = File::create(&action.target)
                    .map_err(|e| Error::Io(std::io::Error::other(e.to_string())))?;
                let mut encoder =
                    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut file, action.quality);
                encoder
                    .encode_image(&img)
                    .map_err(|e| Error::Io(std::io::Error::other(e.to_string())))?;
            } else {
                img.save_with_format(&action.target, action.format)
                    .map_err(|e| Error::Io(std::io::Error::other(e.to_string())))?;
            }

            let _ = writer.copy_metadata(&action.source, &action.target);
        }

        Ok(Outcome { data: () })
    }
}
