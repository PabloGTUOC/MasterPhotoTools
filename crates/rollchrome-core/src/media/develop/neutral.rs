//! Per-make exposure and per-camera colour calibration for neutral RAW development (DV-12).
//!
//! Makes rate their RAW data with different headroom; Apple corrects per make and applies tuned
//! profiles per camera. Calibration aligns neutral development with Apple's rendering.

use rayon::prelude::*;

/// Neutral development calibration parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NeutralCalibration {
    pub exposure_ev: f32,
    pub colour: Option<[[f32; 3]; 3]>,
    /// Lift of the upper tones, in EV at its peak, measured after the standard tone curve: nothing in the
    /// shadows, rising through the midtones to the peak at [`HIGHLIGHT_PEAK`] and back to nothing at white,
    /// so nothing new clips. For a camera whose highlights Apple renders brighter than the standard curve
    /// does, where more exposure would also lift shadows that already match.
    pub highlight_ev: f32,
}

impl Default for NeutralCalibration {
    fn default() -> Self {
        Self {
            exposure_ev: 0.0,
            colour: None,
            highlight_ev: 0.0,
        }
    }
}

// Fitted to Apple's rendering of the owner's six XZ-1 ORFs jointly with the standard tone curve.
// Rows sum to 1 so white stays white.
const XZ1_COLOUR_MATRIX: [[f32; 3]; 3] = [
    [1.20827, -0.16165, -0.04661],
    [0.09254, 0.90609, 0.00137],
    [-0.03717, 0.06336, 0.97382],
];

/// Returns the neutral calibration for a given camera make and model.
///
/// Each value was fitted to Apple's rendering; the LX100 from five files, Nikon and Fujifilm from one file each
/// (a D610 NEF, the X-Pro2 chart RAF), so they are provisional until the owner's D60 and
/// X100VI files are measured; Fujifilm's dynamic-range modes are not handled.
pub fn calibration_for(make: &str, model: &str) -> NeutralCalibration {
    let make_lower = make.to_lowercase();
    if make_lower.contains("olympus") {
        let norm_model: String = model
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '-')
            .collect();
        if norm_model.eq_ignore_ascii_case("xz1") {
            NeutralCalibration {
                exposure_ev: 0.0,
                colour: Some(XZ1_COLOUR_MATRIX),
                highlight_ev: 0.0,
            }
        } else {
            NeutralCalibration {
                exposure_ev: 0.0,
                colour: None,
                highlight_ev: 0.0,
            }
        }
    } else if make_lower.contains("nikon") {
        NeutralCalibration {
            exposure_ev: 0.6,
            colour: None,
            highlight_ev: 0.0,
        }
    } else if make_lower.contains("fujifilm") {
        NeutralCalibration {
            exposure_ev: 1.2,
            colour: None,
            highlight_ev: 0.0,
        }
    } else if make_lower.contains("panasonic") && is_lx100(model) {
        // Fitted to Apple's rendering (`sips`) of the owner's five LX100 RW2s from 2026-11-22, by tone
        // level on pixel-matched images. Exposure alone left the shadows matching and the upper tones
        // 0.02–0.03 short in every file; +0.2 EV with a +0.1 EV highlight lift is the best of a grid
        // (exposure 0.15–0.2, lift 0–0.2): RMS 0.0068 on the four files that line up with Apple's,
        // against 0.0128 with exposure alone, mean within 0.01 EV. P1100539 (14.9 mm, f/11) renders
        // brighter than Apple's throughout and does not line up pixel for pixel; it was not used to
        // choose. Colour is within 1% per channel of Apple's without a matrix. Only the LX100: another
        // Panasonic's baseline is unknown.
        NeutralCalibration {
            exposure_ev: 0.2,
            colour: None,
            highlight_ev: 0.1,
        }
    } else {
        NeutralCalibration::default()
    }
}

/// The LX100 reports itself as `DMC-LX100`; the match ignores case, spaces and hyphens.
fn is_lx100(model: &str) -> bool {
    let norm: String = model
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect();
    norm.eq_ignore_ascii_case("DMCLX100") || norm.eq_ignore_ascii_case("LX100")
}

/// Where the highlight lift starts, peaks and ends, as linear values after the standard tone curve:
/// about 0.45, 0.88 and 1.0 once encoded for the screen. Below the start Apple's shadows already match.
const HIGHLIGHT_START: f32 = 0.15;
pub const HIGHLIGHT_PEAK: f32 = 0.72;
const HIGHLIGHT_END: f32 = 1.0;

/// The highlight lift's weight at a value `t` after the curve: 0 → 1 → 0, smooth at both ends.
fn highlight_weight(t: f32) -> f32 {
    fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
        let u = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
        u * u * (3.0 - 2.0 * u)
    }
    if t <= HIGHLIGHT_START || t >= HIGHLIGHT_END {
        return 0.0;
    }
    let (l0, l1, l2) = (
        HIGHLIGHT_START.log2(),
        HIGHLIGHT_PEAK.log2(),
        HIGHLIGHT_END.log2(),
    );
    let x = t.log2();
    if x <= l1 {
        smooth(l0, l1, x)
    } else {
        1.0 - smooth(l1, l2, x)
    }
}

/// Luminance weights of linear ProPhoto RGB (ROMM, D50).
const PROPHOTO_Y: [f32; 3] = [0.288_040, 0.711_874, 0.000_086];

/// Entries in the table that turns the lift after the curve into a gain before it.
const LIFT_TABLE_LEN: usize = 1024;
/// The table spans the curve's input from 2⁻⁹ (its first knot) to 1, evenly in log2.
const LIFT_TABLE_MIN_LOG2: f32 = -9.0;

/// For each input `v` before the standard tone curve, the gain `g` such that
/// `standard_tone(v · g) = standard_tone(v) · 2^(highlight_ev · weight)`, so the lift is specified where it
/// was measured, after the curve, while being applied with the rest of the calibration, before it. The
/// lifted output never passes 1, so white is untouched.
fn lift_table(highlight_ev: f32) -> Vec<f32> {
    use super::tone::standard_tone;
    (0..LIFT_TABLE_LEN)
        .map(|i| {
            let x = LIFT_TABLE_MIN_LOG2
                + (i as f32) * (-LIFT_TABLE_MIN_LOG2) / ((LIFT_TABLE_LEN - 1) as f32);
            let v = x.exp2();
            let t = standard_tone(v);
            let target = (t * (highlight_ev * highlight_weight(t)).exp2()).min(1.0);
            if target <= t {
                return 1.0;
            }
            // The curve is monotonic: bisect for the input that reaches the target.
            let (mut lo, mut hi) = (v, 1.0f32);
            for _ in 0..40 {
                let mid = 0.5 * (lo + hi);
                if standard_tone(mid) < target {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            0.5 * (lo + hi) / v
        })
        .collect()
}

/// The gain for `v` from [`lift_table`], interpolated in log2; 1 outside the table's span.
fn lift_gain(table: &[f32], v: f32) -> f32 {
    if v.is_nan() || v <= 0.0 || v >= 1.0 {
        return 1.0;
    }
    let pos =
        (v.log2() - LIFT_TABLE_MIN_LOG2) / (-LIFT_TABLE_MIN_LOG2) * ((table.len() - 1) as f32);
    if pos <= 0.0 {
        return 1.0;
    }
    let i = (pos.floor() as usize).min(table.len() - 2);
    let f = pos - i as f32;
    table[i] * (1.0 - f) + table[i + 1] * f
}

/// Applies exposure adjustment, colour correction matrix and highlight lift to linear ProPhoto pixels,
/// before the standard tone curve.
///
/// Multiplies by 2^exposure_ev, applies the 3x3 colour matrix if present, clamps negative values to
/// zero, then lifts the upper tones by a gain taken from each pixel's luminance. Operates in parallel via
/// Rayon.
pub fn apply_neutral(pixels: &mut [[f32; 3]], cal: &NeutralCalibration) {
    if cal.exposure_ev == 0.0 && cal.colour.is_none() && cal.highlight_ev == 0.0 {
        return;
    }
    let exp_scale = 2.0f32.powf(cal.exposure_ev);
    let matrix = cal.colour;
    let lift = (cal.highlight_ev != 0.0).then(|| lift_table(cal.highlight_ev));
    pixels.par_iter_mut().for_each(|p| {
        let mut r = p[0] * exp_scale;
        let mut g = p[1] * exp_scale;
        let mut b = p[2] * exp_scale;
        if let Some(m) = matrix {
            let nr = m[0][0] * r + m[0][1] * g + m[0][2] * b;
            let ng = m[1][0] * r + m[1][1] * g + m[1][2] * b;
            let nb = m[2][0] * r + m[2][1] * g + m[2][2] * b;
            r = nr;
            g = ng;
            b = nb;
        }
        p[0] = r.max(0.0);
        p[1] = g.max(0.0);
        p[2] = b.max(0.0);
        if let Some(table) = &lift {
            // One gain per pixel, from its luminance: lifting each channel by its own gain raised
            // highlight saturation 6–9% above Apple's on the LX100 files, where it had matched.
            let y = PROPHOTO_Y[0] * p[0] + PROPHOTO_Y[1] * p[1] + PROPHOTO_Y[2] * p[2];
            let g = lift_gain(table, y);
            for c in p.iter_mut() {
                *c *= g;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_xz1_gets_its_colour_and_no_exposure() {
        let cal1 = calibration_for("Olympus", "XZ-1");
        assert_eq!(cal1.exposure_ev, 0.0);
        assert_eq!(cal1.colour, Some(XZ1_COLOUR_MATRIX));

        let cal2 = calibration_for("OLYMPUS IMAGING CORP.", "XZ1");
        assert_eq!(cal2.exposure_ev, 0.0);
        assert_eq!(cal2.colour, Some(XZ1_COLOUR_MATRIX));
    }

    #[test]
    fn nikon_and_fujifilm_get_their_exposure() {
        let nikon = calibration_for("NIKON CORPORATION", "D610");
        assert_eq!(nikon.exposure_ev, 0.6);
        assert_eq!(nikon.colour, None);

        let fuji = calibration_for("FUJIFILM", "X-Pro2");
        assert_eq!(fuji.exposure_ev, 1.2);
        assert_eq!(fuji.colour, None);
    }

    #[test]
    fn the_lx100_gets_its_exposure_and_highlight_lift_and_no_colour() {
        for model in ["DMC-LX100", "dmc-lx100", "LX100"] {
            let cal = calibration_for("Panasonic", model);
            assert_eq!(cal.exposure_ev, 0.2, "model {model}");
            assert_eq!(cal.highlight_ev, 0.1, "model {model}");
            assert_eq!(cal.colour, None, "model {model}");
        }
        assert_eq!(
            calibration_for("Panasonic", "DC-LX100M2"),
            NeutralCalibration::default(),
            "another Panasonic keeps the default until it is measured"
        );
    }

    #[test]
    fn an_unknown_camera_gets_nothing() {
        assert_eq!(
            calibration_for("Canon", "EOS R5"),
            NeutralCalibration::default()
        );
        assert_eq!(
            calibration_for("Sony", "A7III"),
            NeutralCalibration::default()
        );
        assert_eq!(
            calibration_for("Olympus", "E-M1"),
            NeutralCalibration {
                exposure_ev: 0.0,
                colour: None,
                highlight_ev: 0.0,
            }
        );
    }

    #[test]
    fn apply_neutral_keeps_grey_grey_and_scales_by_the_exposure() {
        let xz1_cal = calibration_for("Olympus", "XZ-1");
        let mut pixels = vec![[0.5f32, 0.5f32, 0.5f32]];
        apply_neutral(&mut pixels, &xz1_cal);
        let p = pixels[0];
        assert!(
            (p[0] - p[1]).abs() <= 1e-5,
            "R and G differ: r={}, g={}",
            p[0],
            p[1]
        );
        assert!(
            (p[1] - p[2]).abs() <= 1e-5,
            "G and B differ: g={}, b={}",
            p[1],
            p[2]
        );
        assert!(
            (p[0] - p[2]).abs() <= 1e-5,
            "R and B differ: r={}, b={}",
            p[0],
            p[2]
        );

        let exp_cal = NeutralCalibration {
            exposure_ev: 1.0,
            colour: None,
            highlight_ev: 0.0,
        };
        let mut exp_pixels = vec![[0.5f32, 0.5f32, 0.5f32]];
        apply_neutral(&mut exp_pixels, &exp_cal);
        let ep = exp_pixels[0];
        assert!((ep[0] - 1.0).abs() <= 1e-5);
        assert!((ep[1] - 1.0).abs() <= 1e-5);
        assert!((ep[2] - 1.0).abs() <= 1e-5);
    }

    fn lifted(v: f32, highlight_ev: f32) -> f32 {
        let cal = NeutralCalibration {
            exposure_ev: 0.0,
            colour: None,
            highlight_ev,
        };
        let mut px = vec![[v, v, v]];
        apply_neutral(&mut px, &cal);
        px[0][1]
    }

    /// The input before the curve that the curve maps to `t`.
    fn input_for(t: f32) -> f32 {
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        for _ in 0..60 {
            let mid = 0.5 * (lo + hi);
            if crate::media::develop::tone::standard_tone(mid) < t {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        0.5 * (lo + hi)
    }

    #[test]
    fn the_highlight_lift_is_its_size_after_the_curve_at_its_peak() {
        use crate::media::develop::tone::standard_tone;
        let v = input_for(HIGHLIGHT_PEAK);
        let out = standard_tone(lifted(v, 0.1));
        let ev = (out / standard_tone(v)).log2();
        assert!((ev - 0.1).abs() < 0.005, "lift at the peak was {ev} EV");
    }

    #[test]
    fn the_highlight_lift_leaves_shadows_black_and_white_alone() {
        use crate::media::develop::tone::standard_tone;
        for v in [0.0f32, 1e-4, 0.01, input_for(0.1)] {
            assert_eq!(lifted(v, 0.1), v, "shadow {v} moved");
        }
        let white = lifted(1.0, 0.1);
        assert_eq!(white, 1.0);
        let near_white = input_for(0.995);
        assert!(standard_tone(lifted(near_white, 0.1)) <= 1.0);
    }

    #[test]
    fn the_highlight_lift_never_reverses_tones() {
        use crate::media::develop::tone::standard_tone;
        let mut previous = 0.0f32;
        for i in 1..=2000 {
            let v = i as f32 / 2000.0;
            let out = standard_tone(lifted(v, 0.1));
            assert!(
                out >= previous - 1e-6,
                "tone reversed at {v}: {out} < {previous}"
            );
            previous = out;
        }
    }

    #[test]
    fn the_highlight_lift_keeps_grey_grey_and_a_colour_its_hue() {
        let cal = NeutralCalibration {
            exposure_ev: 0.0,
            colour: None,
            highlight_ev: 0.1,
        };
        let mut px = vec![[0.4f32, 0.4, 0.4], [0.6f32, 0.4, 0.2]];
        apply_neutral(&mut px, &cal);
        assert!((px[0][0] - px[0][1]).abs() < 1e-6 && (px[0][1] - px[0][2]).abs() < 1e-6);
        // One gain per pixel: the channels keep their ratios.
        assert!((px[1][0] / px[1][1] - 1.5).abs() < 1e-5);
        assert!((px[1][2] / px[1][1] - 0.5).abs() < 1e-5);
        assert!(px[1][1] > 0.4, "the colour was not lifted");
    }
}
