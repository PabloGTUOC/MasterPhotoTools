use crate::error::Error;
use crate::jobs::{Outcome, Progress, ToolResult};
use crate::media::image_ops::{decode, resize};
use crate::media::meta::ExifWriter;
use crate::tools::{Plan, Skip, Tool};
use image::{DynamicImage, ImageBuffer, Rgba, RgbaImage};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct PrintBorderParams {
    pub paths: Vec<PathBuf>,
    pub long_edge: u32,
    pub short_edge: u32,
    pub corner_radius: u32,
    pub out_dir: PathBuf,
}

pub struct PrintBorderAction {
    pub source: PathBuf,
    pub target: PathBuf,
    pub long_edge: u32,
    pub short_edge: u32,
    pub corner_radius: u32,
}

pub struct PrintBorderTool;

impl Tool for PrintBorderTool {
    type Params = PrintBorderParams;
    type Action = PrintBorderAction;
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

            let name = path.file_stem().unwrap_or_default().to_string_lossy();
            let target = p.out_dir.join(format!("{}.jpg", name));

            actions.push(PrintBorderAction {
                source: path.clone(),
                target,
                long_edge: p.long_edge,
                short_edge: p.short_edge,
                corner_radius: p.corner_radius,
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
            let img = decode(&action.source)?;

            let is_portrait = img.height() > img.width();
            let canvas_w = if is_portrait {
                action.short_edge
            } else {
                action.long_edge
            };
            let canvas_h = if is_portrait {
                action.long_edge
            } else {
                action.short_edge
            };

            // We need some margins. Say 5% margin.
            let margin_x = canvas_w as f32 * 0.05;
            let margin_y = canvas_h as f32 * 0.05;

            let target_w = canvas_w as f32 - 2.0 * margin_x;
            let target_h = canvas_h as f32 - 2.0 * margin_y;

            let w = img.width();
            let h = img.height();

            let ratio = (target_w / w as f32).min(target_h / h as f32);
            let nw = (w as f32 * ratio).round() as u32;
            let nh = (h as f32 * ratio).round() as u32;

            let resized = resize(&img, nw, nh)?;
            let mut rgba = resized.to_rgba8();

            // Apply rounded corners
            apply_rounded_corners(&mut rgba, action.corner_radius);

            let mut canvas: RgbaImage =
                ImageBuffer::from_pixel(canvas_w, canvas_h, Rgba([255, 255, 255, 255]));

            let cx = (canvas_w - nw) / 2;
            let cy = (canvas_h - nh) / 2;

            // Overlay using alpha
            for y in 0..nh {
                for x in 0..nw {
                    let p = rgba.get_pixel(x, y);
                    if p[3] > 0 {
                        let alpha = p[3] as f32 / 255.0;
                        let bg = canvas.get_pixel(cx + x, cy + y);
                        let r = ((1.0 - alpha) * bg[0] as f32 + alpha * p[0] as f32) as u8;
                        let g = ((1.0 - alpha) * bg[1] as f32 + alpha * p[1] as f32) as u8;
                        let b = ((1.0 - alpha) * bg[2] as f32 + alpha * p[2] as f32) as u8;
                        canvas.put_pixel(cx + x, cy + y, Rgba([r, g, b, 255]));
                    }
                }
            }

            let final_rgb = DynamicImage::ImageRgba8(canvas).into_rgb8();
            final_rgb
                .save(&action.target)
                .map_err(|e| Error::Io(std::io::Error::other(e.to_string())))?;

            let _ = writer.copy_metadata(&action.source, &action.target);
        }

        Ok(Outcome { data: () })
    }
}

fn apply_rounded_corners(img: &mut RgbaImage, radius: u32) {
    if radius == 0 {
        return;
    }
    let w = img.width();
    let h = img.height();
    let r2 = (radius * radius) as f32;

    for y in 0..h {
        for x in 0..w {
            let mut cx = None;
            let mut cy = None;

            if x < radius {
                cx = Some(radius as f32 - x as f32);
            } else if x >= w - radius {
                cx = Some(x as f32 - (w - radius - 1) as f32);
            }

            if y < radius {
                cy = Some(radius as f32 - y as f32);
            } else if y >= h - radius {
                cy = Some(y as f32 - (h - radius - 1) as f32);
            }

            if let (Some(dx), Some(dy)) = (cx, cy) {
                let dist2 = dx * dx + dy * dy;
                if dist2 > r2 {
                    // Outside corner
                    img.put_pixel(x, y, Rgba([0, 0, 0, 0]));
                } else if dist2 > r2 - 2.0 * radius as f32 {
                    // Basic anti-aliasing edge blending could go here
                    // but for simplicity, we just leave it hard or lightly alpha'd
                    let alpha = (r2 - dist2) / (2.0 * radius as f32);
                    let mut p = *img.get_pixel(x, y);
                    p[3] = (alpha.clamp(0.0, 1.0) * 255.0) as u8;
                    img.put_pixel(x, y, p);
                }
            }
        }
    }
}
