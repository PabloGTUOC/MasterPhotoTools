//! JPEG encoding.
//!
//! Uses mozjpeg (libjpeg-turbo) rather than the `image` crate's pure-Rust
//! encoder, for two reasons the specification forces:
//!
//! - **Speed.** §9.1 asks for a 24 MP resize and encode in under 150 ms. The
//!   pure-Rust encoder took ~345 ms for the encode alone.
//! - **Encoding options.** The pure-Rust encoder is fixed at 4:2:2 with no
//!   progressive mode, while F4 and F7 specify *no* chroma subsampling and F8
//!   specifies 4:2:0, progressive and optimised.

use crate::error::Error;
use image::DynamicImage;

/// Chroma subsampling, named by the ratio the specification uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ChromaSubsampling {
    /// 4:4:4 — full chroma resolution. F4 and F7's "no chroma subsampling".
    None,
    /// 4:2:2 — chroma halved horizontally.
    Half,
    /// 4:2:0 — chroma halved in both directions. F8.
    Quarter,
}

impl ChromaSubsampling {
    /// The pixel area one chroma sample covers, as mozjpeg wants it.
    fn pixel_sizes(&self) -> (u8, u8) {
        match self {
            ChromaSubsampling::None => (1, 1),
            ChromaSubsampling::Half => (2, 1),
            ChromaSubsampling::Quarter => (2, 2),
        }
    }

    pub fn ratio(&self) -> &'static str {
        match self {
            ChromaSubsampling::None => "4:4:4",
            ChromaSubsampling::Half => "4:2:2",
            ChromaSubsampling::Quarter => "4:2:0",
        }
    }
}

/// How much work the encoder should spend shrinking the file.
///
/// mozjpeg's default profile enables trellis quantisation, which is a second
/// optimisation pass over every block. It buys real size savings and costs real
/// time — on a 10 MP frame the difference is roughly 130 ms against 550 ms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Effort {
    /// libjpeg-turbo's baseline defaults. For paths with a latency budget.
    Fast,
    /// mozjpeg's trellis-quantised profile. For finished deliverables.
    Max,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct JpegOptions {
    pub quality: u8,
    pub subsampling: ChromaSubsampling,
    pub progressive: bool,
    /// Optimise the Huffman tables. Costs a little time, saves bytes.
    pub optimise: bool,
    pub effort: Effort,
}

impl Default for JpegOptions {
    fn default() -> Self {
        Self::deliverable(95)
    }
}

impl JpegOptions {
    /// A finished photograph: full chroma resolution, optimised.
    ///
    /// F4 step 5 and F7 step 5 — "quality 95 with no chroma subsampling".
    pub fn deliverable(quality: u8) -> Self {
        Self {
            quality,
            subsampling: ChromaSubsampling::None,
            progressive: false,
            optimise: true,
            effort: Effort::Max,
        }
    }

    /// A derivative on a latency budget: full chroma, minimal encoder work.
    ///
    /// This is the profile the §9.1 "resize and encode a 24 MP JPEG in under
    /// 150 ms" target describes.
    pub fn fast(quality: u8) -> Self {
        Self {
            quality,
            subsampling: ChromaSubsampling::None,
            progressive: false,
            optimise: false,
            effort: Effort::Fast,
        }
    }

    /// A file meant for distribution: smaller, progressive.
    ///
    /// F8 — "quality 90, 4:2:0 chroma subsampling, progressive, optimised".
    pub fn distributable(quality: u8) -> Self {
        Self {
            quality,
            subsampling: ChromaSubsampling::Quarter,
            progressive: true,
            optimise: true,
            effort: Effort::Max,
        }
    }

    pub fn with_quality(mut self, quality: u8) -> Self {
        self.quality = quality;
        self
    }
}

/// Encode an image to JPEG bytes.
pub fn encode(img: &DynamicImage, options: &JpegOptions) -> Result<Vec<u8>, Error> {
    let rgb = img.to_rgb8();
    let (width, height) = (rgb.width() as usize, rgb.height() as usize);

    if width == 0 || height == 0 {
        return Err(Error::Internal(
            "Cannot encode an image with a zero dimension".into(),
        ));
    }

    // mozjpeg's C internals use setjmp/longjmp for errors; catch_unwind keeps a
    // malformed input from taking the process down with it.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
        || -> std::io::Result<Vec<u8>> {
            let mut compress = mozjpeg::Compress::new(mozjpeg::ColorSpace::JCS_RGB);

            // Must come first: it re-runs jpeg_set_defaults and would reset anything
            // configured before it.
            if options.effort == Effort::Fast {
                compress.set_fastest_defaults();
            }

            compress.set_size(width, height);
            compress.set_quality(options.quality as f32);

            let (h, v) = options.subsampling.pixel_sizes();
            compress.set_chroma_sampling_pixel_sizes((h, v), (h, v));

            if options.progressive {
                compress.set_progressive_mode();
            }
            compress.set_optimize_coding(options.optimise);
            compress.set_optimize_scans(options.optimise && options.progressive);

            let mut started = compress.start_compress(Vec::new())?;
            started.write_scanlines(rgb.as_raw())?;
            started.finish()
        },
    ));

    match result {
        Ok(Ok(bytes)) => Ok(bytes),
        Ok(Err(e)) => Err(Error::Internal(format!("JPEG encode failed: {e}"))),
        Err(_) => Err(Error::Internal(
            "JPEG encoder aborted while compressing".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbImage;

    fn sample(w: u32, h: u32) -> DynamicImage {
        let mut img = RgbImage::new(w, h);
        for (x, y, p) in img.enumerate_pixels_mut() {
            *p = image::Rgb([(x % 256) as u8, (y % 256) as u8, 128]);
        }
        DynamicImage::ImageRgb8(img)
    }

    #[test]
    fn the_subsampling_ratios_map_to_the_right_pixel_sizes() {
        assert_eq!(ChromaSubsampling::None.pixel_sizes(), (1, 1));
        assert_eq!(ChromaSubsampling::Half.pixel_sizes(), (2, 1));
        assert_eq!(ChromaSubsampling::Quarter.pixel_sizes(), (2, 2));

        assert_eq!(ChromaSubsampling::None.ratio(), "4:4:4");
        assert_eq!(ChromaSubsampling::Quarter.ratio(), "4:2:0");
    }

    #[test]
    fn the_named_profiles_match_their_specifications() {
        // F4 step 5, F7 step 5.
        let d = JpegOptions::deliverable(95);
        assert_eq!(d.quality, 95);
        assert_eq!(d.subsampling, ChromaSubsampling::None);
        assert!(d.optimise);

        // F8.
        let s = JpegOptions::distributable(90);
        assert_eq!(s.quality, 90);
        assert_eq!(s.subsampling, ChromaSubsampling::Quarter);
        assert!(s.progressive);
        assert!(s.optimise);
    }

    #[test]
    fn encoding_produces_a_decodable_jpeg_of_the_right_size() {
        let img = sample(64, 48);
        let bytes = encode(&img, &JpegOptions::deliverable(95)).unwrap();

        assert_eq!(&bytes[..2], &[0xFF, 0xD8], "should start with SOI");
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (64, 48));
    }

    #[test]
    fn lower_quality_produces_a_smaller_file() {
        let img = sample(200, 200);
        let high = encode(&img, &JpegOptions::deliverable(95)).unwrap();
        let low = encode(&img, &JpegOptions::deliverable(50)).unwrap();
        assert!(low.len() < high.len(), "{} vs {}", low.len(), high.len());
    }

    #[test]
    fn subsampling_reduces_size_relative_to_full_chroma() {
        let img = sample(400, 400);
        let full = encode(&img, &JpegOptions::deliverable(90)).unwrap();

        let mut quartered = JpegOptions::deliverable(90);
        quartered.subsampling = ChromaSubsampling::Quarter;
        let small = encode(&img, &quartered).unwrap();

        assert!(
            small.len() < full.len(),
            "4:2:0 should be smaller than 4:4:4: {} vs {}",
            small.len(),
            full.len()
        );
    }

    #[test]
    fn a_zero_dimension_is_an_error_not_a_panic() {
        let img = DynamicImage::ImageRgb8(RgbImage::new(0, 10));
        assert!(encode(&img, &JpegOptions::default()).is_err());
    }
}
