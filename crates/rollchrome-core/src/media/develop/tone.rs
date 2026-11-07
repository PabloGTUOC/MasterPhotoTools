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
}
