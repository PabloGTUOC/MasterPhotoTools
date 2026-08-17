//! F5 — contact sheet.

use crate::error::Error;
use crate::jobs::{Outcome, Progress, ToolResult};
use crate::media::{image_ops, text};
use crate::tools::{expand_inputs, Plan, Tool};
use image::{DynamicImage, ImageBuffer, Rgb, RgbImage};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const ACCEPTED: [&str; 8] = ["jpg", "jpeg", "png", "gif", "tif", "tiff", "heic", "heif"];

pub const DEFAULT_COLS: u32 = 4;
pub const DEFAULT_CELL_SIZE: u32 = 300;
pub const DEFAULT_SPACING: u32 = 20;
pub const DEFAULT_MARGIN: u32 = 40;
/// Height of the caption strip beneath each cell, when captions are on.
pub const LABEL_HEIGHT: u32 = 30;
pub const QUALITY: u8 = 95;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SheetBackground {
    White,
    Black,
}

impl SheetBackground {
    fn fill(&self) -> Rgb<u8> {
        match self {
            SheetBackground::White => Rgb([255, 255, 255]),
            SheetBackground::Black => Rgb([0, 0, 0]),
        }
    }

    /// Caption colour inverts to match the background.
    fn caption(&self) -> Rgb<u8> {
        match self {
            SheetBackground::White => Rgb([0, 0, 0]),
            SheetBackground::Black => Rgb([255, 255, 255]),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SheetOrder {
    Filename,
    ModificationDate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactSheetParams {
    pub inputs: Vec<PathBuf>,
    pub recursive: bool,
    pub cols: u32,
    pub cell_size: u32,
    pub spacing: u32,
    pub margin: u32,
    pub captions: bool,
    pub background: SheetBackground,
    pub order: SheetOrder,
    pub out_path: PathBuf,
}

impl ContactSheetParams {
    pub fn new(inputs: Vec<PathBuf>, out_path: PathBuf) -> Self {
        Self {
            inputs,
            recursive: false,
            cols: DEFAULT_COLS,
            cell_size: DEFAULT_CELL_SIZE,
            spacing: DEFAULT_SPACING,
            margin: DEFAULT_MARGIN,
            captions: true,
            background: SheetBackground::White,
            order: SheetOrder::Filename,
            out_path,
        }
    }
}

/// The sheet's geometry, computed from the specification's formulas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SheetLayout {
    pub cols: u32,
    pub rows: u32,
    pub cell_size: u32,
    pub spacing: u32,
    pub margin: u32,
    pub label_height: u32,
    pub width: u32,
    pub height: u32,
}

impl SheetLayout {
    /// ```text
    /// width  = cols × cell_size + (cols − 1) × spacing + 2 × margin
    /// height = rows × (cell_size + label_height) + (rows − 1) × spacing + 2 × margin
    /// ```
    pub fn compute(
        count: u32,
        cols: u32,
        cell_size: u32,
        spacing: u32,
        margin: u32,
        captions: bool,
    ) -> Self {
        let cols = cols.max(1);
        let rows = count.div_ceil(cols).max(1);
        let label_height = if captions { LABEL_HEIGHT } else { 0 };

        Self {
            cols,
            rows,
            cell_size,
            spacing,
            margin,
            label_height,
            width: cols * cell_size + cols.saturating_sub(1) * spacing + 2 * margin,
            height: rows * (cell_size + label_height)
                + rows.saturating_sub(1) * spacing
                + 2 * margin,
        }
    }

    /// Top-left of cell `index`.
    pub fn cell_origin(&self, index: u32) -> (u32, u32) {
        let col = index % self.cols;
        let row = index / self.cols;
        (
            self.margin + col * (self.cell_size + self.spacing),
            self.margin + row * (self.cell_size + self.label_height + self.spacing),
        )
    }

    /// Caption font size: `max(10, cell_size × 0.04)`.
    pub fn caption_size(&self) -> u32 {
        std::cmp::max(10, (self.cell_size as f32 * 0.04).round() as u32)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactSheetAction {
    pub sources: Vec<PathBuf>,
    pub out_path: PathBuf,
    pub layout: SheetLayout,
    pub captions: bool,
    pub background: SheetBackground,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactSheetSummary {
    pub out_path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub cells: usize,
    /// Files that could not be read and were drawn as a crossed box instead.
    pub unreadable: Vec<PathBuf>,
}

pub struct ContactSheetTool;

impl Tool for ContactSheetTool {
    type Params = ContactSheetParams;
    type Action = ContactSheetAction;
    type Summary = ContactSheetSummary;

    /// Dry run. Creates nothing.
    fn plan(&self, p: &Self::Params) -> ToolResult<Plan<Self::Action>> {
        let (mut files, skipped) = expand_inputs(&p.inputs, p.recursive, &ACCEPTED);

        if files.is_empty() {
            return Err(Error::Config(
                "No readable images were supplied for the contact sheet".into(),
            ));
        }

        match p.order {
            SheetOrder::Filename => files.sort_by_key(|f| {
                f.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_lowercase()
            }),
            SheetOrder::ModificationDate => {
                files.sort_by_key(|f| crate::tools::f1_dates::modified_time(f))
            }
        }

        let layout = SheetLayout::compute(
            files.len() as u32,
            p.cols,
            p.cell_size,
            p.spacing,
            p.margin,
            p.captions,
        );

        Ok(Outcome {
            data: Plan {
                actions: vec![ContactSheetAction {
                    sources: files,
                    out_path: p.out_path.clone(),
                    layout,
                    captions: p.captions,
                    background: p.background,
                }],
                skipped,
            },
        })
    }

    fn apply(
        &self,
        plan: Plan<Self::Action>,
        progress: &dyn Progress,
    ) -> ToolResult<Self::Summary> {
        let action = plan
            .actions
            .into_iter()
            .next()
            .ok_or_else(|| Error::Config("Nothing to build".into()))?;

        let layout = action.layout;
        let total = action.sources.len() as u64;
        let mut unreadable = Vec::new();

        let mut sheet: RgbImage =
            ImageBuffer::from_pixel(layout.width, layout.height, action.background.fill());

        for (index, source) in action.sources.iter().enumerate() {
            if progress.cancelled() {
                break;
            }
            progress.report(index as u64, total, &source.to_string_lossy());

            let (cell_x, cell_y) = layout.cell_origin(index as u32);

            // One unreadable file must never abort the sheet.
            match thumbnail(source, layout.cell_size) {
                Ok(thumb) => {
                    let x = cell_x + (layout.cell_size - thumb.width()) / 2;
                    let y = cell_y + (layout.cell_size - thumb.height()) / 2;
                    image::imageops::overlay(&mut sheet, &thumb, x as i64, y as i64);
                }
                Err(_) => {
                    draw_crossed_box(&mut sheet, cell_x, cell_y, layout.cell_size);
                    unreadable.push(source.clone());
                }
            }

            if action.captions {
                draw_caption(
                    &mut sheet,
                    source,
                    &layout,
                    cell_x,
                    cell_y,
                    action.background,
                );
            }
        }

        if let Some(parent) = action.out_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        image_ops::encode_jpeg(&DynamicImage::ImageRgb8(sheet), QUALITY, &action.out_path)?;

        progress.report(total, total, "done");
        Ok(Outcome {
            data: ContactSheetSummary {
                out_path: action.out_path,
                width: layout.width,
                height: layout.height,
                cells: action.sources.len(),
                unreadable,
            },
        })
    }
}

/// Decode and scale one image to fit a cell, honouring EXIF orientation.
fn thumbnail(source: &std::path::Path, cell_size: u32) -> Result<RgbImage, Error> {
    let img = image_ops::decode_oriented(source)?;
    let scaled = image_ops::downscale_to_max_edge(&img, cell_size)?;

    // A smaller image is left at its own size and centred, not stretched.
    Ok(scaled.to_rgb8())
}

/// A red crossed box, for a file that could not be read.
fn draw_crossed_box(sheet: &mut RgbImage, x: u32, y: u32, size: u32) {
    let red = Rgb([220, 30, 30]);
    let last = size.saturating_sub(1);

    let mut put = |px: u32, py: u32| {
        if px < sheet.width() && py < sheet.height() {
            sheet.put_pixel(px, py, red);
        }
    };

    for i in 0..size {
        // Border.
        put(x + i, y);
        put(x + i, y + last);
        put(x, y + i);
        put(x + last, y + i);
        // Both diagonals.
        put(x + i, y + i);
        put(x + i, y + last - i);
    }
}

fn draw_caption(
    sheet: &mut RgbImage,
    source: &std::path::Path,
    layout: &SheetLayout,
    cell_x: u32,
    cell_y: u32,
    background: SheetBackground,
) {
    let name = source.file_name().unwrap_or_default().to_string_lossy();
    let caption = text::shorten_caption(&name);

    let scale = text::scale_for_height(layout.caption_size());
    let width = text::measure(&caption, scale);

    // Centre within the cell, and sit inside the label strip below it.
    let origin_x = cell_x as i64 + (layout.cell_size as i64 - width as i64) / 2;
    let origin_y = cell_y as i64
        + layout.cell_size as i64
        + (layout.label_height as i64 - (text::GLYPH_HEIGHT * scale) as i64) / 2;

    let colour = background.caption();
    let (sheet_w, sheet_h) = (sheet.width() as i64, sheet.height() as i64);

    text::draw(&caption, origin_x, origin_y, scale, |px, py| {
        if px >= 0 && py >= 0 && px < sheet_w && py < sheet_h {
            sheet.put_pixel(px as u32, py as u32, colour);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_width_formula_matches_the_specification() {
        // 4 cols × 300 + 3 × 20 + 2 × 40 = 1200 + 60 + 80 = 1340
        let l = SheetLayout::compute(8, 4, 300, 20, 40, false);
        assert_eq!(l.width, 1340);
    }

    #[test]
    fn the_height_formula_includes_the_label_strip() {
        // 2 rows × (300 + 30) + 1 × 20 + 2 × 40 = 660 + 20 + 80 = 760
        let with = SheetLayout::compute(8, 4, 300, 20, 40, true);
        assert_eq!(with.rows, 2);
        assert_eq!(with.label_height, 30);
        assert_eq!(with.height, 760);

        // Without captions the strip contributes nothing.
        let without = SheetLayout::compute(8, 4, 300, 20, 40, false);
        assert_eq!(without.label_height, 0);
        assert_eq!(without.height, 700);
    }

    #[test]
    fn a_partial_last_row_still_counts_as_a_row() {
        assert_eq!(SheetLayout::compute(9, 4, 300, 20, 40, false).rows, 3);
        assert_eq!(SheetLayout::compute(1, 4, 300, 20, 40, false).rows, 1);
        // Never zero rows, even for an empty sheet.
        assert_eq!(SheetLayout::compute(0, 4, 300, 20, 40, false).rows, 1);
    }

    #[test]
    fn cells_are_laid_out_left_to_right_then_down() {
        let l = SheetLayout::compute(8, 4, 300, 20, 40, true);

        assert_eq!(l.cell_origin(0), (40, 40));
        // One column across: margin + 300 + 20.
        assert_eq!(l.cell_origin(1), (360, 40));
        // Wrapping to the next row: margin + 300 + 30 + 20.
        assert_eq!(l.cell_origin(4), (40, 390));
    }

    #[test]
    fn the_caption_size_has_a_floor_of_ten() {
        // 300 × 0.04 = 12.
        assert_eq!(
            SheetLayout::compute(1, 4, 300, 20, 40, true).caption_size(),
            12
        );
        // A small cell still gets a legible floor.
        assert_eq!(
            SheetLayout::compute(1, 4, 100, 20, 40, true).caption_size(),
            10
        );
        // A large cell scales up.
        assert_eq!(
            SheetLayout::compute(1, 4, 1000, 20, 40, true).caption_size(),
            40
        );
    }

    #[test]
    fn the_caption_colour_inverts_with_the_background() {
        assert_eq!(SheetBackground::White.caption(), Rgb([0, 0, 0]));
        assert_eq!(SheetBackground::Black.caption(), Rgb([255, 255, 255]));
        assert_eq!(SheetBackground::Black.fill(), Rgb([0, 0, 0]));
    }
}
