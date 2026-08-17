//! Decode, encode, orientation and resize. The only place image bytes are touched.

use crate::error::Error;
use crate::media::exif_jpeg;
use crate::media::meta::{read_meta, Orientation};
use fast_image_resize as fr;
use image::{DynamicImage, ImageFormat, ImageReader, RgbImage, RgbaImage};
use std::io::Cursor;
use std::path::Path;

/// The quality ladder F13 steps down until a byte cap is satisfied.
pub const QUALITY_LADDER: [u8; 4] = [95, 88, 82, 75];

/// Decode an image. Orientation is **not** applied — see [`decode_oriented`].
pub fn decode(path: &Path) -> Result<DynamicImage, Error> {
    let reader = ImageReader::open(path)?
        .with_guessed_format()
        .map_err(|e| Error::Internal(format!("Could not identify {}: {e}", path.display())))?;
    reader
        .decode()
        .map_err(|e| Error::Internal(format!("Decode failed for {}: {e}", path.display())))
}

/// Decode an image with its EXIF orientation applied.
///
/// Every tool that works in pixel space wants this: a frame shot in portrait on
/// a camera that records orientation rather than rotating pixels is otherwise
/// processed sideways.
pub fn decode_oriented(path: &Path) -> Result<DynamicImage, Error> {
    let img = decode(path)?;
    let orientation = read_meta(path)
        .map(|m| m.orientation)
        .unwrap_or(Orientation::Normal);
    Ok(apply_orientation(img, orientation))
}

/// Apply an EXIF orientation to decoded pixels.
pub fn apply_orientation(img: DynamicImage, orientation: Orientation) -> DynamicImage {
    match orientation {
        Orientation::Normal => img,
        Orientation::FlipHorizontal => img.fliph(),
        Orientation::Rotate180 => img.rotate180(),
        Orientation::FlipVertical => img.flipv(),
        Orientation::Transpose => img.rotate90().fliph(),
        Orientation::Rotate90 => img.rotate90(),
        Orientation::Transverse => img.rotate270().fliph(),
        Orientation::Rotate270 => img.rotate270(),
    }
}

/// Resize to exact dimensions with SIMD scaling.
///
/// Stays in RGB8 for images without alpha: carrying a fourth channel through a
/// 24 MP resize costs 24 MB of pointless traffic.
pub fn resize(img: &DynamicImage, new_w: u32, new_h: u32) -> Result<DynamicImage, Error> {
    if new_w == 0 || new_h == 0 {
        return Err(Error::Internal(
            "Resize target must have non-zero width and height".into(),
        ));
    }
    if new_w == img.width() && new_h == img.height() {
        return Ok(img.clone());
    }

    let (w, h) = (img.width(), img.height());

    // Borrow the existing buffer where the layout already matches. Materialising
    // an RGB8 copy of a 24 MP frame costs ~70 ms and 72 MB before any scaling
    // has happened, which is half the §9.1 budget spent on a memcpy.
    match img {
        DynamicImage::ImageRgb8(buf) => {
            let src = fr::images::ImageRef::new(w, h, buf.as_raw(), fr::PixelType::U8x3)
                .map_err(|e| Error::Internal(format!("resize source: {e:?}")))?;
            resize_rgb8(&src, new_w, new_h)
        }
        DynamicImage::ImageRgba8(buf) => {
            let src = fr::images::ImageRef::new(w, h, buf.as_raw(), fr::PixelType::U8x4)
                .map_err(|e| Error::Internal(format!("resize source: {e:?}")))?;
            resize_rgba8(&src, new_w, new_h)
        }
        other if other.color().has_alpha() => {
            let owned = other.to_rgba8();
            let src = fr::images::ImageRef::new(w, h, owned.as_raw(), fr::PixelType::U8x4)
                .map_err(|e| Error::Internal(format!("resize source: {e:?}")))?;
            resize_rgba8(&src, new_w, new_h)
        }
        other => {
            let owned = other.to_rgb8();
            let src = fr::images::ImageRef::new(w, h, owned.as_raw(), fr::PixelType::U8x3)
                .map_err(|e| Error::Internal(format!("resize source: {e:?}")))?;
            resize_rgb8(&src, new_w, new_h)
        }
    }
}

fn resize_rgb8(src: &fr::images::ImageRef<'_>, w: u32, h: u32) -> Result<DynamicImage, Error> {
    let mut dst = fr::images::Image::new(w, h, fr::PixelType::U8x3);
    fr::Resizer::new()
        .resize(src, &mut dst, None)
        .map_err(|e| Error::Internal(format!("resize: {e:?}")))?;
    let buf = RgbImage::from_raw(w, h, dst.into_vec())
        .ok_or_else(|| Error::Internal("resize produced a malformed buffer".into()))?;
    Ok(DynamicImage::ImageRgb8(buf))
}

fn resize_rgba8(src: &fr::images::ImageRef<'_>, w: u32, h: u32) -> Result<DynamicImage, Error> {
    let mut dst = fr::images::Image::new(w, h, fr::PixelType::U8x4);
    fr::Resizer::new()
        .resize(src, &mut dst, None)
        .map_err(|e| Error::Internal(format!("resize: {e:?}")))?;
    let buf = RgbaImage::from_raw(w, h, dst.into_vec())
        .ok_or_else(|| Error::Internal("resize produced a malformed buffer".into()))?;
    Ok(DynamicImage::ImageRgba8(buf))
}

/// Scale so the long edge is at most `max_edge`. **Downscale only** — an image
/// already within the limit is returned untouched, never enlarged (F6).
pub fn downscale_to_max_edge(img: &DynamicImage, max_edge: u32) -> Result<DynamicImage, Error> {
    let long = img.width().max(img.height());
    if long <= max_edge || max_edge == 0 {
        return Ok(img.clone());
    }
    let ratio = max_edge as f64 / long as f64;
    let w = ((img.width() as f64 * ratio).floor() as u32).max(1);
    let h = ((img.height() as f64 * ratio).floor() as u32).max(1);
    resize(img, w, h)
}

/// F13's resize formula, preserving aspect ratio:
///
/// ```text
/// scale = sqrt(max_megapixels × 10⁶ / (w × h))
/// w′ = floor(w × scale)
/// h′ = floor(h × scale)
/// ```
///
/// Returns `None` when the image is already within the ceiling.
pub fn dimensions_for_megapixels(w: u32, h: u32, max_megapixels: u32) -> Option<(u32, u32)> {
    let pixels = w as f64 * h as f64;
    let ceiling = max_megapixels as f64 * 1_000_000.0;
    if pixels <= ceiling {
        return None;
    }
    let scale = (ceiling / pixels).sqrt();
    Some((
        ((w as f64 * scale).floor() as u32).max(1),
        ((h as f64 * scale).floor() as u32).max(1),
    ))
}

/// Encode to JPEG bytes at a given quality.
pub fn encode_jpeg_bytes(img: &DynamicImage, quality: u8) -> Result<Vec<u8>, Error> {
    let rgb = DynamicImage::ImageRgb8(img.to_rgb8());
    let mut buf = Vec::new();
    {
        let mut cursor = Cursor::new(&mut buf);
        let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut cursor, quality);
        enc.encode_image(&rgb)
            .map_err(|e| Error::Internal(format!("JPEG encode failed: {e}")))?;
    }
    Ok(buf)
}

/// Encode stepping quality down `95 → 88 → 82 → 75` until the result fits
/// `max_bytes` (F13).
///
/// Returns the encoded bytes and the quality that produced them. If even 75
/// overshoots, the 75 result is returned with `fits = false` — the caller
/// decides whether to resize further, and is never told a cap was met when it
/// was not (§9.2 invariant 6).
pub fn encode_jpeg_within(
    img: &DynamicImage,
    max_bytes: u64,
) -> Result<(Vec<u8>, u8, bool), Error> {
    let mut last = Vec::new();
    let mut last_quality = QUALITY_LADDER[0];

    for quality in QUALITY_LADDER {
        let bytes = encode_jpeg_bytes(img, quality)?;
        let fits = bytes.len() as u64 <= max_bytes;
        last_quality = quality;
        last = bytes;
        if fits {
            return Ok((last, last_quality, true));
        }
    }
    Ok((last, last_quality, false))
}

/// Encode to a file in a given format. JPEG and WebP honour `quality`.
pub fn encode_to(
    img: &DynamicImage,
    path: &Path,
    format: ImageFormat,
    quality: u8,
) -> Result<(), Error> {
    match format {
        ImageFormat::Jpeg => {
            let bytes = encode_jpeg_bytes(img, quality)?;
            std::fs::write(path, bytes)?;
            Ok(())
        }
        other => img
            .save_with_format(path, other)
            .map_err(|e| Error::Internal(format!("Encode failed for {}: {e}", path.display()))),
    }
}

pub fn encode_jpeg(img: &DynamicImage, quality: u8, path: &Path) -> Result<(), Error> {
    encode_to(img, path, ImageFormat::Jpeg, quality)
}

/// **Mandatory (F13).** Re-encode an image, carrying the source JPEG's EXIF
/// forward and updating the recorded pixel dimensions.
///
/// Dropping EXIF here destroys the capture date that was just validated, and
/// Google Photos would then file the photograph under its upload date instead of
/// the date it was taken.
///
/// Returns whether an EXIF block was carried across. A source with no EXIF is
/// not an error, but the caller is told so rather than left to assume.
pub fn reencode_preserving_exif(
    source: &Path,
    img: &DynamicImage,
    destination: &Path,
    quality: u8,
) -> Result<bool, Error> {
    let encoded = encode_jpeg_bytes(img, quality)?;
    let out = carry_exif_forward(source, &encoded, img.width(), img.height())?;
    let carried = out.len() != encoded.len();
    std::fs::write(destination, out)?;
    Ok(carried)
}

/// As [`reencode_preserving_exif`], but stepping quality down to meet a byte cap.
pub fn reencode_preserving_exif_within(
    source: &Path,
    img: &DynamicImage,
    destination: &Path,
    max_bytes: u64,
) -> Result<(u8, bool), Error> {
    // The spliced EXIF block adds to the final size, so measure the real output
    // at each rung rather than the bare JPEG.
    let mut last: Option<(Vec<u8>, u8)> = None;

    for quality in QUALITY_LADDER {
        let encoded = encode_jpeg_bytes(img, quality)?;
        let out = carry_exif_forward(source, &encoded, img.width(), img.height())?;
        let fits = out.len() as u64 <= max_bytes;
        last = Some((out, quality));
        if fits {
            break;
        }
    }

    let (bytes, quality) = last.expect("the ladder is never empty");
    let fits = bytes.len() as u64 <= max_bytes;
    std::fs::write(destination, bytes)?;
    Ok((quality, fits))
}

/// Splice the source's Exif block into freshly encoded JPEG bytes, with the
/// pixel-dimension tags updated to the new size.
fn carry_exif_forward(
    source: &Path,
    encoded: &[u8],
    width: u32,
    height: u32,
) -> Result<Vec<u8>, Error> {
    let original = std::fs::read(source)?;
    match exif_jpeg::extract(&original) {
        Some(mut block) => {
            block.set_pixel_dimensions(width, height);
            Ok(exif_jpeg::splice(encoded, &block))
        }
        None => Ok(encoded.to_vec()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_f13_resize_formula_matches_the_specification() {
        // 24 MP down to a 10 MP ceiling.
        let (w, h) = dimensions_for_megapixels(6000, 4000, 10).unwrap();
        assert!((w as f64 * h as f64) <= 10_000_000.0);
        // Aspect ratio is preserved to within a pixel of rounding.
        let before = 6000.0 / 4000.0;
        let after = w as f64 / h as f64;
        assert!((before - after).abs() < 0.001, "{before} vs {after}");
    }

    #[test]
    fn an_image_within_the_megapixel_ceiling_is_left_alone() {
        assert_eq!(dimensions_for_megapixels(3000, 2000, 10), None);
        // Exactly at the boundary passes.
        assert_eq!(dimensions_for_megapixels(4000, 2500, 10), None);
        // A hair over does not.
        assert!(dimensions_for_megapixels(4000, 2501, 10).is_some());
    }

    #[test]
    fn orientation_is_applied_and_transposing_values_swap_the_axes() {
        let img = DynamicImage::ImageRgb8(RgbImage::new(40, 20));

        let same = apply_orientation(img.clone(), Orientation::Normal);
        assert_eq!((same.width(), same.height()), (40, 20));

        let rotated = apply_orientation(img.clone(), Orientation::Rotate90);
        assert_eq!((rotated.width(), rotated.height()), (20, 40));

        let flipped = apply_orientation(img, Orientation::FlipHorizontal);
        assert_eq!((flipped.width(), flipped.height()), (40, 20));
    }

    #[test]
    fn downscaling_never_enlarges() {
        let img = DynamicImage::ImageRgb8(RgbImage::new(100, 50));
        let out = downscale_to_max_edge(&img, 400).unwrap();
        assert_eq!((out.width(), out.height()), (100, 50));
    }

    #[test]
    fn downscaling_caps_the_long_edge_and_keeps_the_ratio() {
        let img = DynamicImage::ImageRgb8(RgbImage::new(1000, 500));
        let out = downscale_to_max_edge(&img, 200).unwrap();
        assert_eq!((out.width(), out.height()), (200, 100));
    }

    #[test]
    fn a_zero_sized_resize_is_an_error_not_a_silent_passthrough() {
        let img = DynamicImage::ImageRgb8(RgbImage::new(10, 10));
        assert!(resize(&img, 0, 10).is_err());
        assert!(resize(&img, 10, 0).is_err());
    }
}
