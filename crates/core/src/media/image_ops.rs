//! Decode, encode, orientation and resize. The only place image bytes are touched.

use crate::error::Error;
use crate::media::meta::{read_meta, Orientation};
use crate::media::{exif_jpeg, jpeg};
use fast_image_resize as fr;
use image::{DynamicImage, ImageFormat, ImageReader, RgbImage, RgbaImage};
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
/// Returns `None` when the image is already within the ceiling, and when there
/// is no ceiling at all.
///
/// **Zero means no resolution ceiling.** Publishing is limited by file size far
/// more often than by pixel count, and a photographer who wants the full frame
/// kept has to be able to say so. Without this, zero would compute a scale of
/// zero and reduce every image to a single pixel — a magic value that has to be
/// handled rather than merely documented.
pub fn dimensions_for_megapixels(w: u32, h: u32, max_megapixels: u32) -> Option<(u32, u32)> {
    if max_megapixels == 0 {
        return None;
    }
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

/// Rotate by an arbitrary angle, expanding the canvas so nothing is clipped (F6).
///
/// Right angles use the exact rotations; other angles are resampled bilinearly
/// with the uncovered corners left transparent, so a later flatten decides the
/// background rather than this function guessing one.
pub fn rotate_expanding(img: &DynamicImage, degrees: f32) -> DynamicImage {
    let normalised = degrees.rem_euclid(360.0);

    if (normalised - 0.0).abs() < f32::EPSILON {
        return img.clone();
    }
    if (normalised - 90.0).abs() < 0.01 {
        return img.rotate90();
    }
    if (normalised - 180.0).abs() < 0.01 {
        return img.rotate180();
    }
    if (normalised - 270.0).abs() < 0.01 {
        return img.rotate270();
    }

    let src = img.to_rgba8();
    let (w, h) = (src.width() as f32, src.height() as f32);
    let radians = normalised.to_radians();
    let (sin, cos) = radians.sin_cos();

    // The bounding box of the rotated corners.
    let new_w = (w * cos.abs() + h * sin.abs()).ceil().max(1.0);
    let new_h = (w * sin.abs() + h * cos.abs()).ceil().max(1.0);

    let mut out: RgbaImage =
        image::ImageBuffer::from_pixel(new_w as u32, new_h as u32, image::Rgba([0, 0, 0, 0]));

    let (src_cx, src_cy) = (w / 2.0, h / 2.0);
    let (dst_cx, dst_cy) = (new_w / 2.0, new_h / 2.0);

    for y in 0..out.height() {
        for x in 0..out.width() {
            // Inverse map: where in the source does this destination pixel come from?
            let dx = x as f32 + 0.5 - dst_cx;
            let dy = y as f32 + 0.5 - dst_cy;
            let sx = dx * cos + dy * sin + src_cx - 0.5;
            let sy = -dx * sin + dy * cos + src_cy - 0.5;

            if sx < -0.5 || sy < -0.5 || sx > w - 0.5 || sy > h - 0.5 {
                continue;
            }
            out.put_pixel(x, y, sample_bilinear(&src, sx, sy));
        }
    }

    DynamicImage::ImageRgba8(out)
}

fn sample_bilinear(src: &RgbaImage, x: f32, y: f32) -> image::Rgba<u8> {
    let x0 = x.floor().max(0.0) as u32;
    let y0 = y.floor().max(0.0) as u32;
    let x1 = (x0 + 1).min(src.width() - 1);
    let y1 = (y0 + 1).min(src.height() - 1);
    let fx = (x - x0 as f32).clamp(0.0, 1.0);
    let fy = (y - y0 as f32).clamp(0.0, 1.0);

    let mut channels = [0u8; 4];
    for (c, out) in channels.iter_mut().enumerate() {
        let p00 = src.get_pixel(x0, y0)[c] as f32;
        let p10 = src.get_pixel(x1, y0)[c] as f32;
        let p01 = src.get_pixel(x0, y1)[c] as f32;
        let p11 = src.get_pixel(x1, y1)[c] as f32;
        let top = p00 + (p10 - p00) * fx;
        let bottom = p01 + (p11 - p01) * fx;
        *out = (top + (bottom - top) * fy).round().clamp(0.0, 255.0) as u8;
    }
    image::Rgba(channels)
}

/// Flatten any alpha onto an opaque background colour.
pub fn flatten_onto(img: &DynamicImage, background: [u8; 3]) -> DynamicImage {
    if !img.color().has_alpha() {
        return DynamicImage::ImageRgb8(img.to_rgb8());
    }
    let src = img.to_rgba8();
    let mut out = RgbImage::new(src.width(), src.height());
    for (x, y, pixel) in src.enumerate_pixels() {
        let a = pixel[3] as f32 / 255.0;
        let blended = [
            ((1.0 - a) * background[0] as f32 + a * pixel[0] as f32).round() as u8,
            ((1.0 - a) * background[1] as f32 + a * pixel[1] as f32).round() as u8,
            ((1.0 - a) * background[2] as f32 + a * pixel[2] as f32).round() as u8,
        ];
        out.put_pixel(x, y, image::Rgb(blended));
    }
    DynamicImage::ImageRgb8(out)
}

/// Encode to JPEG bytes at a given quality, with full chroma resolution.
///
/// See [`crate::media::jpeg`] for the subsampling and progressive controls the
/// specification's per-tool output rules need.
pub fn encode_jpeg_bytes(img: &DynamicImage, quality: u8) -> Result<Vec<u8>, Error> {
    jpeg::encode(img, &jpeg::JpegOptions::fast(quality))
}

/// Encode to JPEG bytes with explicit options.
pub fn encode_jpeg_with(img: &DynamicImage, options: &jpeg::JpegOptions) -> Result<Vec<u8>, Error> {
    jpeg::encode(img, options)
}

/// Write a JPEG file with explicit options.
pub fn write_jpeg_with(
    img: &DynamicImage,
    path: &Path,
    options: &jpeg::JpegOptions,
) -> Result<(), Error> {
    std::fs::write(path, jpeg::encode(img, options)?)?;
    Ok(())
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
