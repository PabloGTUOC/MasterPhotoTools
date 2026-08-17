use crate::error::Error;
use crate::jobs::{Outcome, Progress, ToolResult};
use crate::media::image_ops::decode;
use crate::media::meta::ExifWriter;
use crate::tools::{Plan, Skip, Tool};

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct SplitParams {
    pub paths: Vec<PathBuf>,
    pub out_dir: PathBuf,
}

pub struct SplitAction {
    pub source: PathBuf,
    pub target_left: PathBuf,
    pub target_right: PathBuf,
}

pub struct SplitTool;

impl Tool for SplitTool {
    type Params = SplitParams;
    type Action = SplitAction;
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
            let ext = path.extension().unwrap_or_default().to_string_lossy();

            let target_left = p.out_dir.join(format!("{}_1.{}", name, ext));
            let target_right = p.out_dir.join(format!("{}_2.{}", name, ext));

            actions.push(SplitAction {
                source: path.clone(),
                target_left,
                target_right,
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

            // 1. Convert to grayscale to find the divider
            let luma = img.to_luma8();
            let (w, h) = luma.dimensions();

            if w < h {
                return Err(Error::Internal(
                    "Expected a landscape image for half-frame splitting".into(),
                ));
            }

            // Look in the middle 30% of the image (from 35% to 65%)
            let start_x = (w as f32 * 0.35) as u32;
            let end_x = (w as f32 * 0.65) as u32;

            let mut min_brightness = f32::MAX;
            let mut best_col = w / 2;

            for x in start_x..end_x {
                let mut sum = 0u32;
                for y in 0..h {
                    sum += luma.get_pixel(x, y)[0] as u32;
                }
                let avg = sum as f32 / h as f32;
                if avg < min_brightness {
                    min_brightness = avg;
                    best_col = x;
                }
            }

            // 2. Split
            // We assume the divider has some width. We'll cut 1% off from the center column
            let gap = (w as f32 * 0.01) as u32;

            // Left image
            let left_w = best_col.saturating_sub(gap);
            let left_img = img.crop_imm(0, 0, left_w, h);

            // Right image
            let right_start = best_col + gap;
            let right_w = w.saturating_sub(right_start);
            let right_img = img.crop_imm(right_start, 0, right_w, h);

            left_img
                .save(&action.target_left)
                .map_err(|e| Error::Io(std::io::Error::other(e.to_string())))?;
            right_img
                .save(&action.target_right)
                .map_err(|e| Error::Io(std::io::Error::other(e.to_string())))?;

            let _ = writer.copy_metadata(&action.source, &action.target_left);
            let _ = writer.copy_metadata(&action.source, &action.target_right);
        }

        Ok(Outcome { data: () })
    }
}
