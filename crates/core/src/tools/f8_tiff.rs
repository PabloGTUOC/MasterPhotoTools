//! F8 — TIFF to JPEG.

use crate::error::Error;
use crate::jobs::{Outcome, Progress, ToolResult};
use crate::media::image_ops;
use crate::tools::{expand_inputs, Plan, Tool};
use image::{DynamicImage, ImageBuffer};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const ACCEPTED: [&str; 2] = ["tif", "tiff"];
pub const DEFAULT_MAX_LONG_EDGE: u32 = 2048;
pub const DEFAULT_QUALITY: u8 = 90;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TiffToJpegParams {
    pub inputs: Vec<PathBuf>,
    pub recursive: bool,
    pub max_long_edge: u32,
    pub quality: u8,
    pub out_dir: PathBuf,
}

impl TiffToJpegParams {
    pub fn new(inputs: Vec<PathBuf>, out_dir: PathBuf) -> Self {
        Self {
            inputs,
            recursive: false,
            max_long_edge: DEFAULT_MAX_LONG_EDGE,
            quality: DEFAULT_QUALITY,
            out_dir,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TiffToJpegAction {
    pub source: PathBuf,
    /// Output directory plus the source stem; page suffixes are appended at apply.
    pub target_base: PathBuf,
    pub max_long_edge: u32,
    pub quality: u8,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TiffToJpegSummary {
    pub written: Vec<PathBuf>,
    pub failures: Vec<(PathBuf, String)>,
}

/// Name for page `index` of `pages` total.
///
/// A single-page TIFF produces `{base}.jpg`; a multi-page one produces
/// `{base}_p001.jpg`, `{base}_p002.jpg` and so on.
pub fn page_name(base: &Path, index: usize, pages: usize) -> PathBuf {
    if pages <= 1 {
        return base.with_extension("jpg");
    }
    let stem = base.file_name().unwrap_or_default().to_string_lossy();
    let parent = base.parent().unwrap_or(Path::new("."));
    parent.join(format!("{stem}_p{:03}.jpg", index + 1))
}

/// Decode every page of a TIFF.
///
/// The `image` crate exposes only the first page, so the `tiff` crate is driven
/// directly. Colour types beyond the common ones are reported rather than
/// guessed at.
pub fn decode_pages(path: &Path) -> Result<Vec<DynamicImage>, Error> {
    use tiff::decoder::Decoder;

    let file = std::fs::File::open(path)?;
    let mut decoder = Decoder::new(std::io::BufReader::new(file))
        .map_err(|e| Error::Internal(format!("Not a readable TIFF: {e}")))?;

    let mut pages = Vec::new();
    loop {
        let (width, height) = decoder
            .dimensions()
            .map_err(|e| Error::Internal(format!("TIFF dimensions unreadable: {e}")))?;
        let colour = decoder
            .colortype()
            .map_err(|e| Error::Internal(format!("TIFF colour type unreadable: {e}")))?;
        let data = decoder
            .read_image()
            .map_err(|e| Error::Internal(format!("TIFF page could not be decoded: {e}")))?;

        pages.push(to_dynamic(width, height, colour, data)?);

        match decoder.next_image() {
            Ok(()) => continue,
            Err(_) => break,
        }
    }

    if pages.is_empty() {
        return Err(Error::Internal("TIFF contained no pages".into()));
    }
    Ok(pages)
}

fn to_dynamic(
    width: u32,
    height: u32,
    colour: tiff::ColorType,
    data: tiff::decoder::DecodingResult,
) -> Result<DynamicImage, Error> {
    use tiff::decoder::DecodingResult;
    use tiff::ColorType;

    let unsupported = |what: &str| Error::Internal(format!("Unsupported TIFF format: {what}"));

    match (colour, data) {
        (ColorType::RGB(8), DecodingResult::U8(buf)) => ImageBuffer::from_raw(width, height, buf)
            .map(DynamicImage::ImageRgb8)
            .ok_or_else(|| unsupported("RGB8 buffer size")),
        (ColorType::RGBA(8), DecodingResult::U8(buf)) => ImageBuffer::from_raw(width, height, buf)
            .map(DynamicImage::ImageRgba8)
            .ok_or_else(|| unsupported("RGBA8 buffer size")),
        (ColorType::Gray(8), DecodingResult::U8(buf)) => ImageBuffer::from_raw(width, height, buf)
            .map(DynamicImage::ImageLuma8)
            .ok_or_else(|| unsupported("Gray8 buffer size")),
        (ColorType::GrayA(8), DecodingResult::U8(buf)) => ImageBuffer::from_raw(width, height, buf)
            .map(DynamicImage::ImageLumaA8)
            .ok_or_else(|| unsupported("GrayA8 buffer size")),

        // 16-bit scanner output is common; narrow it rather than refuse it.
        (ColorType::RGB(16), DecodingResult::U16(buf)) => {
            let narrowed: Vec<u8> = buf.iter().map(|v| (v >> 8) as u8).collect();
            ImageBuffer::from_raw(width, height, narrowed)
                .map(DynamicImage::ImageRgb8)
                .ok_or_else(|| unsupported("RGB16 buffer size"))
        }
        (ColorType::Gray(16), DecodingResult::U16(buf)) => {
            let narrowed: Vec<u8> = buf.iter().map(|v| (v >> 8) as u8).collect();
            ImageBuffer::from_raw(width, height, narrowed)
                .map(DynamicImage::ImageLuma8)
                .ok_or_else(|| unsupported("Gray16 buffer size"))
        }

        (other, _) => Err(unsupported(&format!("{other:?}"))),
    }
}

pub struct TiffToJpegTool;

impl Tool for TiffToJpegTool {
    type Params = TiffToJpegParams;
    type Action = TiffToJpegAction;
    type Summary = TiffToJpegSummary;

    /// Dry run. Creates nothing.
    fn plan(&self, p: &Self::Params) -> ToolResult<Plan<Self::Action>> {
        let (files, skipped) = expand_inputs(&p.inputs, p.recursive, &ACCEPTED);

        let actions = files
            .into_iter()
            .map(|source| {
                let stem = source.file_stem().unwrap_or_default().to_string_lossy();
                TiffToJpegAction {
                    target_base: p.out_dir.join(stem.to_string()),
                    source,
                    max_long_edge: p.max_long_edge,
                    quality: p.quality,
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
        let mut summary = TiffToJpegSummary::default();

        for (done, action) in plan.actions.into_iter().enumerate() {
            if progress.cancelled() {
                break;
            }
            progress.report(done as u64, total, &action.source.to_string_lossy());

            match convert_one(&action) {
                Ok(mut written) => summary.written.append(&mut written),
                Err(e) => summary.failures.push((action.source, e.to_string())),
            }
        }

        progress.report(total, total, "done");
        Ok(Outcome { data: summary })
    }
}

fn convert_one(action: &TiffToJpegAction) -> Result<Vec<PathBuf>, Error> {
    if let Some(parent) = action.target_base.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let pages = decode_pages(&action.source)?;
    let count = pages.len();
    let mut written = Vec::with_capacity(count);

    for (index, page) in pages.into_iter().enumerate() {
        // Alpha is flattened onto white, never dropped to black.
        let flattened = image_ops::flatten_onto(&page, [255, 255, 255]);
        let capped = image_ops::downscale_to_max_edge(&flattened, action.max_long_edge)?;

        let target = page_name(&action.target_base, index, count);
        image_ops::encode_jpeg(&capped, action.quality, &target)?;
        written.push(target);
    }

    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_page_tiff_keeps_the_plain_name() {
        let base = PathBuf::from("/out/scan");
        assert_eq!(page_name(&base, 0, 1), PathBuf::from("/out/scan.jpg"));
    }

    #[test]
    fn a_multi_page_tiff_numbers_its_pages_from_one() {
        let base = PathBuf::from("/out/scan");
        assert_eq!(page_name(&base, 0, 3), PathBuf::from("/out/scan_p001.jpg"));
        assert_eq!(page_name(&base, 1, 3), PathBuf::from("/out/scan_p002.jpg"));
        assert_eq!(page_name(&base, 2, 3), PathBuf::from("/out/scan_p003.jpg"));
    }

    #[test]
    fn page_numbers_are_zero_padded_to_three_digits() {
        let base = PathBuf::from("/out/big");
        assert_eq!(page_name(&base, 9, 200), PathBuf::from("/out/big_p010.jpg"));
        assert_eq!(
            page_name(&base, 99, 200),
            PathBuf::from("/out/big_p100.jpg")
        );
    }

    #[test]
    fn the_defaults_match_the_specification() {
        assert_eq!(DEFAULT_MAX_LONG_EDGE, 2048);
        assert_eq!(DEFAULT_QUALITY, 90);
    }
}
