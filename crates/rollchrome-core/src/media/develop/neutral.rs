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
}

impl Default for NeutralCalibration {
    fn default() -> Self {
        Self {
            exposure_ev: 0.0,
            colour: None,
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
/// Each value was fitted to Apple's rendering; Nikon and Fujifilm from one file each
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
            }
        } else {
            NeutralCalibration {
                exposure_ev: 0.0,
                colour: None,
            }
        }
    } else if make_lower.contains("nikon") {
        NeutralCalibration {
            exposure_ev: 0.6,
            colour: None,
        }
    } else if make_lower.contains("fujifilm") {
        NeutralCalibration {
            exposure_ev: 1.2,
            colour: None,
        }
    } else {
        NeutralCalibration::default()
    }
}

/// Applies exposure adjustment and colour correction matrix to linear ProPhoto pixels.
///
/// Multiplies by 2^exposure_ev, applies the 3x3 colour matrix if present, and clamps
/// negative values to zero. Operates in parallel via Rayon.
pub fn apply_neutral(pixels: &mut [[f32; 3]], cal: &NeutralCalibration) {
    if cal.exposure_ev == 0.0 && cal.colour.is_none() {
        return;
    }
    let exp_scale = 2.0f32.powf(cal.exposure_ev);
    let matrix = cal.colour;
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
                colour: None
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
        };
        let mut exp_pixels = vec![[0.5f32, 0.5f32, 0.5f32]];
        apply_neutral(&mut exp_pixels, &exp_cal);
        let ep = exp_pixels[0];
        assert!((ep[0] - 1.0).abs() <= 1e-5);
        assert!((ep[1] - 1.0).abs() <= 1e-5);
        assert!((ep[2] - 1.0).abs() <= 1e-5);
    }
}
