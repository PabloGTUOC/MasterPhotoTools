//! Histogram for the Edit tab's preview frame.
//!
//! Reads the display-encoded RGBA8 image — the values the person sees after the sRGB transfer —
//! not linear light, so the bins line up with what is on screen.

use crate::error::Error;
use serde::{Deserialize, Serialize};

/// Per-channel and luminance histogram for an 8-bit RGBA preview frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Histogram {
    /// 256 bins, one per red code value.
    pub red: Vec<u32>,
    /// 256 bins, one per green code value.
    pub green: Vec<u32>,
    /// 256 bins, one per blue code value.
    pub blue: Vec<u32>,
    /// 256 bins of Rec. 709 luma computed in integer arithmetic.
    pub luminance: Vec<u32>,
    /// Number of pixels that contributed to the histogram.
    pub pixels: u32,
    /// Pixels with any channel at 255 — a value of 1.0, at or above the plan's 0.999.
    pub highlight_clipped: u32,
    /// Pixels with any channel at 0 — a value of 0.0, at or below the plan's 0.001.
    pub shadow_clipped: u32,
}

impl Histogram {
    /// True if at least one pixel has a channel at the display white point.
    pub fn highlights_clipped(&self) -> bool {
        self.highlight_clipped != 0
    }

    /// True if at least one pixel has a channel at the display black point.
    pub fn shadows_clipped(&self) -> bool {
        self.shadow_clipped != 0
    }
}

/// Computes a histogram from a display-encoded RGBA8 frame.
///
/// Alpha is ignored. A zero-sized frame is valid and yields all-zero bins.
///
/// # Errors
///
/// Returns `Error::Refused` when `rgba` is not exactly `width * height * 4` bytes.
pub fn histogram(rgba: &[u8], width: u32, height: u32) -> Result<Histogram, Error> {
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4));

    let expected_len = match expected {
        Some(len) => len,
        None => {
            return Err(Error::Refused(format!(
                "frame dimensions {}x{} overflow the byte count",
                width, height
            )))
        }
    };

    if rgba.len() != expected_len {
        return Err(Error::Refused(format!(
            "expected {} bytes for a {}x{} RGBA frame, but got {}",
            expected_len,
            width,
            height,
            rgba.len()
        )));
    }

    let mut red = vec![0u32; 256];
    let mut green = vec![0u32; 256];
    let mut blue = vec![0u32; 256];
    let mut luminance = vec![0u32; 256];
    let mut highlight_clipped = 0u32;
    let mut shadow_clipped = 0u32;

    // The plan's clipping thresholds are <= 0.001 and >= 0.999 on a 0-1 scale.
    // In 8-bit codes that is exactly 0 and 255: 1/255 = 0.0039, and 254/255 = 0.996.
    // A "fix" to 254 would be wrong, so the thresholds are intentionally inclusive.
    for pixel in rgba.chunks_exact(4) {
        let r = pixel[0] as u32;
        let g = pixel[1] as u32;
        let b = pixel[2] as u32;

        red[r as usize] += 1;
        green[g as usize] += 1;
        blue[b as usize] += 1;

        // Integer Rec. 709 luma keeps the bin deterministic; a float would make the
        // result depend on rounding, and tests would have to guess.
        let luma = (2126 * r + 7152 * g + 722 * b + 5000) / 10000;
        luminance[luma as usize] += 1;

        if r == 255 || g == 255 || b == 255 {
            highlight_clipped += 1;
        }
        if r == 0 || g == 0 || b == 0 {
            shadow_clipped += 1;
        }
    }

    let pixels = width.saturating_mul(height);

    Ok(Histogram {
        red,
        green,
        blue,
        luminance,
        pixels,
        highlight_clipped,
        shadow_clipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn histogram_bins_match_frame_pixels_exactly() {
        // Two pixels: (10, 20, 30) and (40, 50, 60).
        let rgba = vec![10, 20, 30, 255, 40, 50, 60, 255];
        let hist = histogram(&rgba, 2, 1).unwrap();

        assert_eq!(hist.pixels, 2);
        assert_eq!(hist.red[10], 1);
        assert_eq!(hist.red[40], 1);
        assert_eq!(hist.green[20], 1);
        assert_eq!(hist.green[50], 1);
        assert_eq!(hist.blue[30], 1);
        assert_eq!(hist.blue[60], 1);

        let luma0 = (2126 * 10 + 7152 * 20 + 722 * 30 + 5000) / 10000;
        let luma1 = (2126 * 40 + 7152 * 50 + 722 * 60 + 5000) / 10000;
        assert_eq!(hist.luminance[luma0 as usize], 1);
        assert_eq!(hist.luminance[luma1 as usize], 1);
    }

    #[test]
    fn clipping_flags_detect_crushed_blacks_and_blown_highlights() {
        let highlight = histogram(&[255, 10, 10, 255], 1, 1).unwrap();
        assert!(highlight.highlights_clipped());
        assert!(!highlight.shadows_clipped());

        let shadow = histogram(&[0, 128, 128, 255], 1, 1).unwrap();
        assert!(!shadow.highlights_clipped());
        assert!(shadow.shadows_clipped());

        let neither = histogram(&[254, 1, 128, 255], 1, 1).unwrap();
        assert!(!neither.highlights_clipped());
        assert!(!neither.shadows_clipped());

        let midtones = histogram(&[128, 128, 128, 255, 64, 96, 192, 255], 2, 1).unwrap();
        assert!(!midtones.highlights_clipped());
        assert!(!midtones.shadows_clipped());
    }

    #[test]
    fn every_channel_sums_to_the_pixel_count() {
        let rgba: Vec<u8> = (0..64)
            .flat_map(|i| [i as u8, (i * 3) as u8, (i * 7) as u8, 255])
            .collect();
        let hist = histogram(&rgba, 8, 8).unwrap();

        assert_eq!(hist.red.iter().sum::<u32>(), hist.pixels);
        assert_eq!(hist.green.iter().sum::<u32>(), hist.pixels);
        assert_eq!(hist.blue.iter().sum::<u32>(), hist.pixels);
        assert_eq!(hist.luminance.iter().sum::<u32>(), hist.pixels);
    }

    #[test]
    fn alpha_does_not_change_the_result() {
        let opaque = histogram(&[100, 150, 200, 255], 1, 1).unwrap();
        let transparent = histogram(&[100, 150, 200, 0], 1, 1).unwrap();
        assert_eq!(opaque, transparent);
    }

    #[test]
    fn a_frame_of_the_wrong_length_is_refused() {
        let err = histogram(&[255, 255, 255], 1, 1).unwrap_err();
        match err {
            Error::Refused(msg) => {
                assert!(msg.contains("expected 4 bytes"), "got: {msg}");
                assert!(msg.contains("got 3"), "got: {msg}");
            }
            other => panic!("expected Error::Refused, got {other:?}"),
        }

        let err = histogram(&[0; 9], 2, 1).unwrap_err();
        match err {
            Error::Refused(msg) => {
                assert!(msg.contains("expected 8 bytes"), "got: {msg}");
                assert!(msg.contains("got 9"), "got: {msg}");
            }
            other => panic!("expected Error::Refused, got {other:?}"),
        }
    }

    #[test]
    fn a_zero_sized_frame_is_valid() {
        let hist = histogram(&[], 0, 0).unwrap();
        assert_eq!(hist.pixels, 0);
        assert!(hist.red.iter().all(|&c| c == 0));
        assert!(hist.green.iter().all(|&c| c == 0));
        assert!(hist.blue.iter().all(|&c| c == 0));
        assert!(hist.luminance.iter().all(|&c| c == 0));
        assert!(!hist.highlights_clipped());
        assert!(!hist.shadows_clipped());

        let hist = histogram(&[], 0, 5).unwrap();
        assert_eq!(hist.pixels, 0);
        assert_eq!(hist.red.iter().sum::<u32>(), 0);
    }

    #[test]
    fn luminance_weights_green_most_and_blue_least() {
        let red = histogram(&[255, 0, 0, 255], 1, 1).unwrap();
        let green = histogram(&[0, 255, 0, 255], 1, 1).unwrap();
        let blue = histogram(&[0, 0, 255, 255], 1, 1).unwrap();

        let expected_red = (2126 * 255 + 5000) / 10000;
        let expected_green = (7152 * 255 + 5000) / 10000;
        let expected_blue = (722 * 255 + 5000) / 10000;

        assert_eq!(red.luminance[expected_red as usize], 1);
        assert_eq!(green.luminance[expected_green as usize], 1);
        assert_eq!(blue.luminance[expected_blue as usize], 1);

        assert_eq!(expected_red, 54);
        assert_eq!(expected_green, 182);
        assert_eq!(expected_blue, 18);
    }
}
