use crate::error::Error;
use crate::jobs::{Outcome, Progress, ToolResult};
use crate::media::image_ops::{decode, resize};
use crate::tools::{Plan, Skip, Tool};
use image::{ImageBuffer, Rgba, RgbaImage};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct ContactSheetParams {
    pub paths: Vec<PathBuf>,
    pub cols: u32,
    pub cell_size: u32,
    pub spacing: u32,
    pub margin: u32,
    pub out_path: PathBuf,
}

pub struct ContactSheetAction {
    pub paths: Vec<PathBuf>,
    pub out_path: PathBuf,
    pub cols: u32,
    pub cell_size: u32,
    pub spacing: u32,
    pub margin: u32,
}

pub struct ContactSheetTool;

impl Tool for ContactSheetTool {
    type Params = ContactSheetParams;
    type Action = ContactSheetAction;
    type Summary = ();

    fn plan(&self, p: &Self::Params) -> ToolResult<Plan<Self::Action>> {
        let mut skipped = Vec::new();
        let mut valid_paths = Vec::new();

        for path in &p.paths {
            if !path.exists() {
                skipped.push(Skip {
                    file: path.to_string_lossy().into(),
                    reason: "File not found".into(),
                });
            } else {
                valid_paths.push(path.clone());
            }
        }

        if valid_paths.is_empty() {
            return Err(Error::Internal("No valid files for contact sheet".into()));
        }

        let action = ContactSheetAction {
            paths: valid_paths,
            out_path: p.out_path.clone(),
            cols: p.cols,
            cell_size: p.cell_size,
            spacing: p.spacing,
            margin: p.margin,
        };

        Ok(Outcome {
            data: Plan {
                actions: vec![action],
                skipped,
            },
        })
    }

    fn apply(
        &self,
        plan: Plan<Self::Action>,
        _progress: &dyn Progress,
    ) -> ToolResult<Self::Summary> {
        if plan.actions.is_empty() {
            return Ok(Outcome { data: () });
        }

        let action = &plan.actions[0];

        let count = action.paths.len() as u32;
        let rows = (count as f32 / action.cols as f32).ceil() as u32;

        let width =
            action.margin * 2 + action.cols * action.cell_size + (action.cols - 1) * action.spacing;
        let height = action.margin * 2 + rows * action.cell_size + (rows - 1) * action.spacing;

        let mut sheet: RgbaImage =
            ImageBuffer::from_pixel(width, height, Rgba([255, 255, 255, 255]));

        for (i, path) in action.paths.iter().enumerate() {
            let i = i as u32;
            let col = i % action.cols;
            let row = i / action.cols;

            let x = action.margin + col * (action.cell_size + action.spacing);
            let y = action.margin + row * (action.cell_size + action.spacing);

            let thumb = match decode(path) {
                Ok(img) => {
                    // Resize to fit inside cell_size
                    let w = img.width();
                    let h = img.height();
                    let ratio = (action.cell_size as f64) / (w.max(h) as f64);
                    let nw = (w as f64 * ratio).round() as u32;
                    let nh = (h as f64 * ratio).round() as u32;
                    match resize(&img, nw, nh) {
                        Ok(resized) => resized.to_rgba8(),
                        Err(_) => generate_error_thumb(action.cell_size),
                    }
                }
                Err(_) => generate_error_thumb(action.cell_size),
            };

            // Center thumbnail in cell
            let cx = x + (action.cell_size - thumb.width()) / 2;
            let cy = y + (action.cell_size - thumb.height()) / 2;

            image::imageops::overlay(&mut sheet, &thumb, cx as i64, cy as i64);
        }

        sheet
            .save(&action.out_path)
            .map_err(|e| Error::Io(std::io::Error::other(e.to_string())))?;

        Ok(Outcome { data: () })
    }
}

fn generate_error_thumb(size: u32) -> RgbaImage {
    let mut img = ImageBuffer::from_pixel(size, size, Rgba([240, 240, 240, 255]));
    let red = Rgba([255, 0, 0, 255]);
    for i in 0..size {
        img.put_pixel(i, i, red);
        img.put_pixel(i, size - 1 - i, red);
        img.put_pixel(i, 0, red);
        img.put_pixel(i, size - 1, red);
        img.put_pixel(0, i, red);
        img.put_pixel(size - 1, i, red);
    }
    img
}
