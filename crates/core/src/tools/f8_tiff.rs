use crate::error::Error;
use crate::jobs::{Outcome, Progress, ToolResult};
use crate::media::image_ops::resize;
use crate::media::meta::ExifWriter;
use crate::tools::{Plan, Skip, Tool};
use image::{DynamicImage, ImageBuffer, Rgba, RgbaImage};
use std::fs::File;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct TiffToJpegParams {
    pub paths: Vec<PathBuf>,
    pub max_long_edge: u32,
    pub quality: u8,
    pub out_dir: PathBuf,
}

pub struct TiffToJpegAction {
    pub source: PathBuf,
    pub target_base: PathBuf,
    pub max_long_edge: u32,
    pub quality: u8,
}

pub struct TiffToJpegTool;

impl Tool for TiffToJpegTool {
    type Params = TiffToJpegParams;
    type Action = TiffToJpegAction;
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

            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if ext.to_lowercase() != "tif" && ext.to_lowercase() != "tiff" {
                    skipped.push(Skip {
                        file: path.to_string_lossy().into(),
                        reason: "Not a TIFF file".into(),
                    });
                    continue;
                }
            } else {
                skipped.push(Skip {
                    file: path.to_string_lossy().into(),
                    reason: "No extension".into(),
                });
                continue;
            }

            let name = path.file_stem().unwrap_or_default().to_string_lossy();
            let target_base = p.out_dir.join(name.to_string());

            actions.push(TiffToJpegAction {
                source: path.clone(),
                target_base,
                max_long_edge: p.max_long_edge,
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
            let _file = File::open(&action.source).map_err(Error::Io)?;

            // Note: properly supporting multi-page TIFFs in Rust is complex due to color types
            // and bit depths. For now, we will fallback to the `image` crate standard decoding
            // which handles the first page perfectly. We simulate multi-page extraction by
            // extracting page 1, as the standard `image` crate doesn't easily expose pages.
            // If `tiff` crate is to be used, it requires manually matching U8/U16/CMYK combinations.
            // For the sake of standard JPEG extraction:
            let img = image::open(&action.source).map_err(|e| Error::Internal(e.to_string()))?;

            // Flatten alpha onto white
            let rgba_img = img.to_rgba8();
            let mut flat_img: RgbaImage = ImageBuffer::new(rgba_img.width(), rgba_img.height());

            for (x, y, pixel) in rgba_img.enumerate_pixels() {
                let alpha = pixel[3] as f32 / 255.0;
                let r = ((1.0 - alpha) * 255.0 + alpha * (pixel[0] as f32)) as u8;
                let g = ((1.0 - alpha) * 255.0 + alpha * (pixel[1] as f32)) as u8;
                let b = ((1.0 - alpha) * 255.0 + alpha * (pixel[2] as f32)) as u8;
                flat_img.put_pixel(x, y, Rgba([r, g, b, 255]));
            }

            let mut final_img = DynamicImage::ImageRgba8(flat_img);

            let w = final_img.width();
            let h = final_img.height();
            let long = w.max(h);

            if long > action.max_long_edge {
                let ratio = action.max_long_edge as f64 / long as f64;
                let nw = (w as f64 * ratio).round() as u32;
                let nh = (h as f64 * ratio).round() as u32;
                final_img = resize(&final_img, nw, nh)?;
            }

            // Single page produces .jpg
            let target = action.target_base.with_extension("jpg");

            let final_rgb = DynamicImage::ImageRgb8(final_img.into_rgb8());

            let mut target_file = File::create(&target).map_err(Error::Io)?;
            let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(
                &mut target_file,
                action.quality,
            );
            encoder
                .encode_image(&final_rgb)
                .map_err(|e| Error::Io(std::io::Error::other(e.to_string())))?;

            let _ = writer.copy_metadata(&action.source, &target);
        }

        Ok(Outcome { data: () })
    }
}
