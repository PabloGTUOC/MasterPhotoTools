//! F5 — contact sheet.

use crate::error::Error;
use crate::jobs::{Outcome, Progress, ToolResult};
use crate::media::jpeg::JpegOptions;
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

/// Frames to a strip when the sheet is laid out as film.
///
/// Five is how a 36-exposure roll is normally cut for filing — six strips of
/// six, or seven of five — and it is what fits an A4 sheet at a readable size.
pub const FRAMES_PER_STRIP: u32 = 5;

/// 35mm film, in millimetres, so the drawing keeps the real proportions.
///
/// Everything below is derived from the frame width, which is the one number
/// the caller chooses. A sheet that gets these ratios wrong stops reading as
/// film however carefully the sprockets are drawn.
mod film {
    /// The image area: 36 × 24 mm, the classic 3:2.
    pub const FRAME_W_MM: f32 = 36.0;
    pub const FRAME_H_MM: f32 = 24.0;
    /// Full film width is 35 mm, leaving 5.5 mm of rebate above and below.
    pub const REBATE_MM: f32 = 5.5;
    /// The gap between one frame and the next.
    pub const FRAME_GAP_MM: f32 = 2.0;
    /// Perforations: 1.98 × 2.79 mm on a 4.75 mm pitch, eight to a frame.
    pub const PERF_W_MM: f32 = 2.79;
    pub const PERF_H_MM: f32 = 1.98;
    pub const PERF_PITCH_MM: f32 = 4.75;
}

/// The paper the strips are laid on.
const PAPER: [u8; 3] = [10, 10, 15];
/// The film base — lighter than the paper, so a perforation reads as a hole.
const FILM_BASE: [u8; 3] = [32, 32, 38];
/// Edge printing: frame numbers, in the warm ink film manufacturers use.
const EDGE_INK: [u8; 3] = [255, 179, 71];
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

/// How the sheet is laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SheetStyle {
    /// The specification's grid: uniform cells, filename captions (F5).
    #[default]
    Grid,
    /// Strips of film on paper, as a contact print is made: frames flush
    /// within a strip, rebate and perforations top and bottom, frame numbers
    /// printed on the edge.
    Filmstrip,
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
    pub style: SheetStyle,
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
            style: SheetStyle::Grid,
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
    pub style: SheetStyle,
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
                    style: p.style,
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

        if action.style == SheetStyle::Filmstrip {
            return render_filmstrip(action, progress);
        }

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
        // Output at JPEG quality 95, optimised.
        image_ops::write_jpeg_with(
            &DynamicImage::ImageRgb8(sheet),
            &action.out_path,
            &JpegOptions::deliverable(QUALITY),
        )?;

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

/// Build the sheet as strips of film on paper.
///
/// The frame is fixed at 3:2 whatever the picture is, because that is what the
/// gate in a camera is. A portrait shot is fitted inside it and centred, with
/// film base either side: nothing is cropped, so the sheet can still be
/// trusted for judging a frame — which is the whole purpose of a proof sheet.
fn render_filmstrip(
    action: ContactSheetAction,
    progress: &dyn Progress,
) -> ToolResult<ContactSheetSummary> {
    let total = action.sources.len() as u64;
    let mut unreadable = Vec::new();

    let layout = StripLayout::compute(
        action.sources.len(),
        FRAMES_PER_STRIP,
        action.layout.cell_size,
        action.layout.margin,
    );

    let mut sheet: RgbImage = ImageBuffer::from_pixel(layout.width, layout.height, Rgb(PAPER));

    for strip in 0..layout.strips {
        draw_strip(
            &mut sheet,
            &layout,
            strip,
            layout.frames_on(strip, action.sources.len()),
        );
    }

    for (index, source) in action.sources.iter().enumerate() {
        if progress.cancelled() {
            break;
        }
        progress.report(index as u64, total, &source.to_string_lossy());

        let (fx, fy) = layout.frame_origin(index as u32);

        match frame_image(source, layout.frame_w, layout.frame_h) {
            Ok(thumb) => {
                let x = fx + (layout.frame_w - thumb.width()) / 2;
                let y = fy + (layout.frame_h - thumb.height()) / 2;
                image::imageops::overlay(&mut sheet, &thumb, x as i64, y as i64);
            }
            Err(_) => {
                // F5's rule holds here too: one bad file marks its frame and
                // the sheet is still built.
                draw_crossed_box(&mut sheet, fx, fy, layout.frame_h.min(layout.frame_w));
                unreadable.push(source.clone());
            }
        }

        draw_frame_number(&mut sheet, &layout, index as u32, index + 1);
    }

    if let Some(parent) = action.out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    image_ops::write_jpeg_with(
        &DynamicImage::ImageRgb8(sheet),
        &action.out_path,
        &JpegOptions::deliverable(QUALITY),
    )?;

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

/// Decode and fit one picture inside a 3:2 frame, honouring EXIF orientation.
fn frame_image(source: &std::path::Path, frame_w: u32, frame_h: u32) -> Result<RgbImage, Error> {
    let img = image_ops::decode_oriented(source)?;
    // Fit within both dimensions rather than the long edge alone: a panorama
    // and a portrait have different limiting sides.
    let scaled = img.resize(frame_w, frame_h, image::imageops::FilterType::Lanczos3);
    Ok(scaled.to_rgb8())
}

/// The geometry of a sheet laid out as strips of film.
///
/// Every measurement is the real 35mm one scaled by the frame width, so the
/// rebate, the perforations and their pitch stay in proportion to the picture
/// whatever size the sheet is asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StripLayout {
    pub frames_per_strip: u32,
    pub strips: u32,
    pub frame_w: u32,
    pub frame_h: u32,
    pub rebate: u32,
    pub frame_gap: u32,
    pub perf_w: u32,
    pub perf_h: u32,
    pub perf_pitch: u32,
    pub margin: u32,
    pub strip_gap: u32,
    pub width: u32,
    pub height: u32,
}

impl StripLayout {
    pub fn compute(count: usize, frames_per_strip: u32, frame_w: u32, margin: u32) -> Self {
        let per = frames_per_strip.max(1);
        let mm = |v: f32| ((frame_w as f32 / film::FRAME_W_MM) * v).round().max(1.0) as u32;

        let frame_h = mm(film::FRAME_H_MM);
        let rebate = mm(film::REBATE_MM);
        let frame_gap = mm(film::FRAME_GAP_MM);

        let strips = (count as u32).div_ceil(per).max(1);
        let strip_h = frame_h + 2 * rebate;
        // Strips are filed with a little air between them, not butted.
        let strip_gap = rebate;

        // A lead-in and lead-out of one gap, so the outer frames are not flush
        // with the cut edge of the strip. The widest strip sets the sheet's
        // width; a short final strip simply ends earlier.
        let widest = (count as u32).min(per).max(1);
        let strip_w = widest * frame_w + (widest + 1) * frame_gap;

        Self {
            frames_per_strip: per,
            strips,
            frame_w,
            frame_h,
            rebate,
            frame_gap,
            perf_w: mm(film::PERF_W_MM),
            perf_h: mm(film::PERF_H_MM),
            perf_pitch: mm(film::PERF_PITCH_MM),
            margin,
            strip_gap,
            width: strip_w + 2 * margin,
            height: strips * strip_h + strips.saturating_sub(1) * strip_gap + 2 * margin,
        }
    }

    /// Top-left of strip `index`.
    pub fn strip_origin(&self, index: u32) -> (u32, u32) {
        let strip_h = self.frame_h + 2 * self.rebate;
        (
            self.margin,
            self.margin + index * (strip_h + self.strip_gap),
        )
    }

    /// Frames on strip `index` — the last one is short unless the count
    /// divides exactly.
    pub fn frames_on(&self, index: u32, count: usize) -> u32 {
        let taken = index * self.frames_per_strip;
        (count as u32)
            .saturating_sub(taken)
            .min(self.frames_per_strip)
    }

    /// How wide a strip carrying `frames` is.
    ///
    /// A strip is cut to what it holds: a roll of four does not leave a fifth
    /// slot of bare film, and a sheet that draws one stops looking like film.
    pub fn strip_width(&self, frames: u32) -> u32 {
        frames * self.frame_w + (frames + 1) * self.frame_gap
    }

    /// Top-left of the image area of frame `index` within the whole sheet.
    pub fn frame_origin(&self, index: u32) -> (u32, u32) {
        let (sx, sy) = self.strip_origin(index / self.frames_per_strip);
        let column = index % self.frames_per_strip;
        (
            sx + self.frame_gap + column * (self.frame_w + self.frame_gap),
            sy + self.rebate,
        )
    }
}

/// Fill a rectangle, clipped to the sheet.
fn fill_rect(sheet: &mut RgbImage, x: u32, y: u32, w: u32, h: u32, colour: [u8; 3]) {
    let colour = Rgb(colour);
    for py in y..(y + h).min(sheet.height()) {
        for px in x..(x + w).min(sheet.width()) {
            sheet.put_pixel(px, py, colour);
        }
    }
}

/// Draw one strip's film base and its two rows of perforations.
fn draw_strip(sheet: &mut RgbImage, layout: &StripLayout, index: u32, frames: u32) {
    let (x, y) = layout.strip_origin(index);
    let strip_h = layout.frame_h + 2 * layout.rebate;
    let strip_w = layout.strip_width(frames);

    fill_rect(sheet, x, y, strip_w, strip_h, FILM_BASE);

    // Perforations run the whole length at a fixed pitch, independently of
    // where the frames fall — which is exactly how film is manufactured, and
    // why they do not line up with the frame edges.
    let inset = (layout.rebate.saturating_sub(layout.perf_h)) / 2;
    let mut px = x + layout.perf_pitch / 2;
    while px + layout.perf_w <= x + strip_w {
        fill_rect(sheet, px, y + inset, layout.perf_w, layout.perf_h, PAPER);
        fill_rect(
            sheet,
            px,
            y + strip_h - inset - layout.perf_h,
            layout.perf_w,
            layout.perf_h,
            PAPER,
        );
        px += layout.perf_pitch;
    }
}

/// Print a frame number on the rebate, as the manufacturer does.
fn draw_frame_number(sheet: &mut RgbImage, layout: &StripLayout, index: u32, number: usize) {
    let (fx, fy) = layout.frame_origin(index);
    let label = format!("{number}");

    let scale = text::scale_for_height(layout.rebate.saturating_sub(2).max(6));
    let width = text::measure(&label, scale);
    let glyph_h = text::GLYPH_HEIGHT * scale;

    let origin_x = fx as i64 + (layout.frame_w as i64 - width as i64) / 2;
    let origin_y = fy as i64 + layout.frame_h as i64 + (layout.rebate as i64 - glyph_h as i64) / 2;

    let (sheet_w, sheet_h) = (sheet.width() as i64, sheet.height() as i64);
    let ink = Rgb(EDGE_INK);
    text::draw(&label, origin_x, origin_y, scale, |px, py| {
        if px >= 0 && py >= 0 && px < sheet_w && py < sheet_h {
            sheet.put_pixel(px as u32, py as u32, ink);
        }
    });
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

#[cfg(test)]
mod filmstrip_tests {
    use super::*;

    /// The strip keeps 35mm's proportions: a 3:2 frame, and a full film width
    /// of 35mm against a 36mm frame.
    #[test]
    fn a_strip_holds_the_real_proportions_of_35mm() {
        let l = StripLayout::compute(5, FRAMES_PER_STRIP, 360, 40);

        assert_eq!(l.frame_w, 360);
        assert_eq!(l.frame_h, 240, "24mm against 36mm is 3:2");
        assert_eq!(l.rebate, 55, "5.5mm of rebate either side");
        assert_eq!(
            l.frame_h + 2 * l.rebate,
            350,
            "35mm of film against a 36mm frame"
        );
    }

    /// Five to a strip, and a sixth picture starts the next one.
    #[test]
    fn frames_wrap_onto_a_second_strip_after_five() {
        let l = StripLayout::compute(6, FRAMES_PER_STRIP, 360, 40);
        assert_eq!(l.strips, 2);

        let (_, first_y) = l.frame_origin(0);
        let (_, fifth_y) = l.frame_origin(4);
        let (_, sixth_y) = l.frame_origin(5);

        assert_eq!(first_y, fifth_y, "the first five share a strip");
        assert!(sixth_y > fifth_y, "the sixth is on the strip below");
    }

    /// Frames sit inside the rebate, never over the perforations.
    #[test]
    fn the_image_area_never_reaches_the_perforations() {
        let l = StripLayout::compute(5, FRAMES_PER_STRIP, 360, 40);
        let (_, strip_y) = l.strip_origin(0);
        let (_, frame_y) = l.frame_origin(0);

        assert_eq!(frame_y, strip_y + l.rebate);
        assert!(l.perf_h < l.rebate, "a perforation fits within the rebate");
    }
}
