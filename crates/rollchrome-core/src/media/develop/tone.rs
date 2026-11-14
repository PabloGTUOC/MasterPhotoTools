//! Base tone curve and ProPhoto (ROMM) transfer functions (DV-1).
//!
//! The base tone maps linear scene-referred luminance to display values. It has
//! three key design properties:
//! 1. A linear segment with a gentle lift (+0.32 EV, slope c = 1.25) up to k = 0.6.
//! 2. A quadratic shoulder for 0.6 < v < 1.0 that meets the linear segment in both
//!    value (0.75) and first derivative (1.25) at v = 0.6.
//! 3. It reaches 1.0 with zero slope (derivative = 0) exactly at sensor clip (v = 1.0),
//!    ensuring clipped skies approach white smoothly without harsh thresholds or flat greys.

/// Maps linear scene-referred value `v` through the fixed base tone curve.
///
/// Constants:
/// - k = 0.6
/// - c = 2.0 / (1.0 + k) = 1.25
/// - A = c / (2.0 * (1.0 - k)) = 1.5625
#[inline]
pub fn base_tone(v: f32) -> f32 {
    if v <= 0.0 {
        0.0
    } else if v <= 0.6 {
        1.25 * v
    } else if v < 1.0 {
        let diff = 1.0 - v;
        1.0 - 1.5625 * diff * diff
    } else {
        1.0
    }
}

/// Knot table for standard tone curve interpolation in log2–log2 space (DV-11).
///
/// Each entry holds `(y_i, m_i)` where `y_i = log2(out_i)` and `m_i` is the
/// PCHIP (Fritsch–Carlson) slope `dy/dx` in log2 space at knot `x_i = i - 9.0`.
const STANDARD_TONE_KNOTS: [(f32, f32); 10] = [
    (-9.361713, 0.363181),
    (-8.783092, 0.735609),
    (-7.77359, 1.120145),
    (-6.515563, 1.283743),
    (-5.205031, 1.41351),
    (-3.67098, 1.391069),
    (-2.398513, 1.132482),
    (-1.378269, 0.794696),
    (-0.7274512, 0.687004),
    (0.0, 0.765768),
];

/// Maps linear scene-referred value `v` through the camera-standard tone curve (DV-11).
///
/// Fitted to the camera JPEGs embedded in the owner's six XZ-1 ORFs on 2026-11-14 by pooling
/// the camera's output per stop. Neutral development with `base_tone` (designed as a flat base
/// for built-in looks) looked flat and muted compared to camera output. A camera or RAW viewer
/// applies an S-curve.
///
/// Applied per channel in linear ProPhoto RGB because cameras apply their tone curve per channel;
/// applying it to luminance alone leaves saturation 8% short, whereas per-channel application
/// restores rich color and matches camera output (blacks 0.051 vs 0.046 camera, whites 0.599 vs 0.601,
/// contrast 0.164 vs 0.166, saturation 0.476 vs 0.472).
#[inline]
pub fn standard_tone(v: f32) -> f32 {
    if v.is_nan() || v <= 0.0 {
        return 0.0;
    }
    if v >= 1.0 {
        return 1.0;
    }
    const FIRST_KNOT_V: f32 = 1.0 / 512.0; // 2^-9 = 0.001953125
    const FIRST_KNOT_OUT: f32 = 0.00152;
    if v < FIRST_KNOT_V {
        return v * (FIRST_KNOT_OUT / FIRST_KNOT_V);
    }

    let x = v.log2();
    let idx = (x + 9.0).floor() as usize;
    let i = idx.min(8);
    let x_i = (i as f32) - 9.0;
    let t = (x - x_i).clamp(0.0, 1.0);

    let (y_i, m_i) = STANDARD_TONE_KNOTS[i];
    let (y_next, m_next) = STANDARD_TONE_KNOTS[i + 1];

    let t2 = t * t;
    let t3 = t2 * t;

    let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
    let h10 = t3 - 2.0 * t2 + t;
    let h01 = -2.0 * t3 + 3.0 * t2;
    let h11 = t3 - t2;

    let y = h00 * y_i + h10 * m_i + h01 * y_next + h11 * m_next;
    y.exp2()
}

/// ROMM (ProPhoto RGB) transfer function encoding linear float to gamma-space [0.0, 1.0].
///
/// - For v < 1/512: 16.0 * v
/// - For v >= 1/512: v^(1/1.8)
/// - Clamped to 0.0..=1.0.
#[inline]
pub fn prophoto_encode(v: f32) -> f32 {
    if v <= 0.0 {
        0.0
    } else if v < 1.0 / 512.0 {
        (16.0 * v).clamp(0.0, 1.0)
    } else {
        v.powf(1.0 / 1.8).clamp(0.0, 1.0)
    }
}

/// Inverse ROMM (ProPhoto RGB) decoding from gamma-space [0.0, 1.0] back to linear float.
///
/// - For y < (1/512) * 16 = 1/32: y / 16.0
/// - For y >= 1/32: y^1.8
/// - Clamped to 0.0..=1.0.
#[inline]
pub fn prophoto_decode(y: f32) -> f32 {
    if y <= 0.0 {
        0.0
    } else if y < 1.0 / 32.0 {
        (y / 16.0).clamp(0.0, 1.0)
    } else {
        y.powf(1.8).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_base_tone_is_pinned() {
        let points = [
            (0.0, 0.0),
            (0.18, 0.225),
            (0.6, 0.75),
            (0.8, 0.9375),
            (1.0, 1.0),
            (2.0, 1.0),
        ];
        for (v, expected) in points {
            let actual = base_tone(v);
            assert!(
                (actual - expected).abs() <= 1e-6,
                "base_tone({}) = {}, expected {}",
                v,
                actual,
                expected
            );
        }
    }

    #[test]
    fn the_base_tone_meets_its_shoulder_smoothly() {
        let h = 1e-5;
        let v_left = 0.6 - h;
        let v_right = 0.6 + h;
        let val_left = base_tone(v_left);
        let val_right = base_tone(v_right);
        assert!(
            (val_left - val_right).abs() < 1e-4,
            "Values around 0.6 disagree: left={}, right={}",
            val_left,
            val_right
        );

        let slope_left = (base_tone(0.6) - base_tone(0.6 - h)) / h;
        let slope_right = (base_tone(0.6 + h) - base_tone(0.6)) / h;
        assert!(
            (slope_left - slope_right).abs() < 1e-4,
            "Slopes around 0.6 disagree: left={}, right={}",
            slope_left,
            slope_right
        );

        let slope_clip = (base_tone(1.0) - base_tone(1.0 - h)) / h;
        assert!(
            slope_clip < 0.01,
            "Slope near clip too high: {}",
            slope_clip
        );
    }

    #[test]
    fn the_base_tone_never_decreases() {
        let mut prev = base_tone(0.0);
        let steps = 4000;
        for i in 1..=steps {
            let v = i as f32 * 1e-3;
            let current = base_tone(v);
            assert!(
                current >= prev,
                "base_tone decreased at v={}: current={}, prev={}",
                v,
                current,
                prev
            );
            prev = current;
        }
    }

    #[test]
    fn prophoto_encoding_follows_romm_and_round_trips() {
        let threshold = 1.0 / 512.0;
        let v_below = threshold - 1e-6;
        let v_above = threshold + 1e-6;
        let enc_below = prophoto_encode(v_below);
        let enc_above = prophoto_encode(v_above);
        assert!(
            (enc_below - enc_above).abs() < 1e-4,
            "Discontinuity at threshold: below={}, above={}",
            enc_below,
            enc_above
        );

        let mut v = 0.0f32;
        while v <= 1.0 {
            let encoded = prophoto_encode(v);
            let decoded = prophoto_decode(encoded);
            assert!(
                (decoded - v).abs() <= 1e-6,
                "Round trip mismatch for v={}: enc={}, dec={}",
                v,
                encoded,
                decoded
            );
            v += 0.001;
        }
    }

    #[test]
    fn the_standard_tone_passes_through_its_knots() {
        let knots = [
            (2.0f32.powi(-9), 0.00152),
            (2.0f32.powi(-8), 0.00227),
            (2.0f32.powi(-7), 0.00457),
            (2.0f32.powi(-6), 0.01093),
            (2.0f32.powi(-5), 0.02711),
            (2.0f32.powi(-4), 0.07851),
            (2.0f32.powi(-3), 0.18966),
            (2.0f32.powi(-2), 0.38468),
            (2.0f32.powi(-1), 0.60397),
            (1.0f32, 1.0),
        ];
        for (v, expected) in knots {
            let actual = standard_tone(v);
            let rel = (actual - expected).abs() / expected;
            assert!(
                rel <= 1e-5,
                "standard_tone({v}) = {actual}, expected {expected}, relative error {rel} > 1e-5"
            );
        }
    }

    #[test]
    fn the_standard_tone_matches_its_reference_values() {
        let refs = [
            (0.001, 0.000778),
            (0.003, 0.001892),
            (0.01, 0.006138),
            (0.05, 0.056245),
            (0.18, 0.283427),
            (0.35, 0.483558),
            (0.75, 0.805839),
            (0.95, 0.961615),
        ];
        for (v, expected) in refs {
            let actual = standard_tone(v);
            let rel = (actual - expected).abs() / expected;
            // 0.000778 has 3 significant figures; rounding 0.00077824 to 6 decimal places gives
            // 0.000778, which has relative rounding error 3.08e-4. The rest match within 2e-4.
            let limit = if (v - 0.001).abs() < 1e-6 {
                3.5e-4
            } else {
                2e-4
            };
            assert!(
                rel <= limit,
                "standard_tone({v}) = {actual}, expected {expected}, rel={rel} > {limit}"
            );
        }
    }

    #[test]
    fn the_standard_tone_never_decreases_and_is_bounded() {
        assert_eq!(standard_tone(0.0), 0.0);
        assert_eq!(standard_tone(-1.0), 0.0);
        assert_eq!(standard_tone(f32::NAN), 0.0);
        assert_eq!(standard_tone(1.0), 1.0);
        assert_eq!(standard_tone(1.2), 1.0);

        let mut prev = standard_tone(0.0);
        let steps = 10_000;
        for step in 1..=steps {
            let v = 1.2 * (step as f32) / (steps as f32);
            let current = standard_tone(v);
            assert!(!current.is_nan(), "standard_tone({v}) produced NaN");
            assert!(
                current >= prev,
                "standard_tone decreased at v={v}: current={current}, prev={prev}"
            );
            if v >= 1.0 {
                assert_eq!(current, 1.0, "standard_tone({v}) != 1.0 at or above 1.0");
            }
            prev = current;
        }
    }

    #[test]
    fn the_standard_tone_darkens_shadows_and_lifts_midtones_against_the_base() {
        assert!(
            standard_tone(0.01) < base_tone(0.01),
            "standard_tone(0.01) [{}] must darken shadows against base_tone(0.01) [{}]",
            standard_tone(0.01),
            base_tone(0.01)
        );
        assert!(
            standard_tone(0.18) > base_tone(0.18),
            "standard_tone(0.18) [{}] must lift midtones against base_tone(0.18) [{}]",
            standard_tone(0.18),
            base_tone(0.18)
        );
    }
}
