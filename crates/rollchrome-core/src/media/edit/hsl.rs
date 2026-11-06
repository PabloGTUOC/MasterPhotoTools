//! 8-band HSL selective color adjustments in OkLCh color space (ED-11).
//!
//! Citations:
//! - Björn Ottosson (2020), "A perceptual color space for image processing" (Oklab / OkLCh).
//! - Joblove & Greenberg (1978), "Color spaces for computer graphics", extended with
//!   smooth raised-cosine windowing across OkLCh hue circle with exact partition of unity.
//!
//! Invariants:
//! 1. **OkLCh Band Centres**: Knots are positioned at the exact OkLCh hue angles of pure reference
//!    sRGB colors (Red, Orange, Yellow, Green, Aqua, Blue, Purple, Magenta).
//! 2. **Partition of Unity**: Window functions sum to identically 1.0 at every angle on the circle
//!    (\sum_{i=0}^7 w_i(h) = 1.0 \forall h \in [0, 2\pi)).
//! 3. **Low-Chroma Fade**: Effects fade smoothly to 0 near C = 0 (neutral grey and near-greys) via
//!    cubic Hermite transition, preventing sensor noise amplification and color flipping.
//! 4. **Unified Oklab Stage**: Evaluated alongside global hue in a single Oklab pass.

#![allow(clippy::excessive_precision)]

use serde::{Deserialize, Serialize};

/// Number of hue bands.
pub const BAND_COUNT: usize = 8;

/// Reference sRGB colors used to establish band center angles in OkLCh.
pub const REFERENCE_SRGB: [(&str, [f32; 3]); BAND_COUNT] = [
    ("Red", [1.0, 0.0, 0.0]),
    ("Orange", [1.0, 0.5, 0.0]),
    ("Yellow", [1.0, 1.0, 0.0]),
    ("Green", [0.0, 1.0, 0.0]),
    ("Aqua", [0.0, 1.0, 1.0]),
    ("Blue", [0.0, 0.0, 1.0]),
    ("Purple", [0.5, 0.0, 1.0]),
    ("Magenta", [1.0, 0.0, 1.0]),
];

/// Exact OkLCh hue knot angles in degrees for each reference color.
/// Computed via standard sRGB -> linear -> Oklab -> OkLCh hue angle.
pub const BAND_HUES_DEG: [f32; BAND_COUNT] = [
    29.233885, // Red
    52.775837, // Orange
    109.76928, // Yellow
    142.49533, // Green
    194.76901, // Aqua
    264.05203, // Blue
    293.77402, // Purple
    328.3634,  // Magenta
];

/// Low-chroma fade start threshold (below this, effect is 0%).
pub const CHROMA_FADE_MIN: f32 = 0.005;

/// Low-chroma fade end threshold (above this, effect is 100%).
pub const CHROMA_FADE_MAX: f32 = 0.025;

/// Smooth cubic Hermite fade factor [0.0, 1.0] as a function of OkLCh chroma.
#[inline(always)]
pub fn chroma_fade(chroma: f32) -> f32 {
    if chroma <= CHROMA_FADE_MIN {
        0.0
    } else if chroma >= CHROMA_FADE_MAX {
        1.0
    } else {
        let t = (chroma - CHROMA_FADE_MIN) * (1.0 / (CHROMA_FADE_MAX - CHROMA_FADE_MIN));
        t * t * (3.0 - 2.0 * t)
    }
}

/// Adjustments for an individual color band.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct HslBand {
    /// Hue shift in degrees: -180.0 to +180.0 (0.0 = identity).
    #[serde(default)]
    pub hue: f32,
    /// Saturation scale percent: -100.0 to +100.0 (0.0 = identity).
    #[serde(default)]
    pub saturation: f32,
    /// Luminance offset percent: -100.0 to +100.0 (0.0 = identity).
    #[serde(default)]
    pub luminance: f32,
}

impl HslBand {
    /// Returns true if this band has all adjustments at 0.0.
    #[inline]
    pub fn is_identity(&self) -> bool {
        self.hue == 0.0 && self.saturation == 0.0 && self.luminance == 0.0
    }
}

/// 8-band selective color adjustments in OkLCh.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct HslAdjustments {
    #[serde(default)]
    pub red: HslBand,
    #[serde(default)]
    pub orange: HslBand,
    #[serde(default)]
    pub yellow: HslBand,
    #[serde(default)]
    pub green: HslBand,
    #[serde(default)]
    pub aqua: HslBand,
    #[serde(default)]
    pub blue: HslBand,
    #[serde(default)]
    pub purple: HslBand,
    #[serde(default)]
    pub magenta: HslBand,
}

impl HslAdjustments {
    /// Array of bands in canonical order: Red, Orange, Yellow, Green, Aqua, Blue, Purple, Magenta.
    pub fn bands(&self) -> [HslBand; BAND_COUNT] {
        [
            self.red,
            self.orange,
            self.yellow,
            self.green,
            self.aqua,
            self.blue,
            self.purple,
            self.magenta,
        ]
    }

    /// True if all 8 bands are at rest (0.0 identity).
    pub fn is_identity(&self) -> bool {
        self.red.is_identity()
            && self.orange.is_identity()
            && self.yellow.is_identity()
            && self.green.is_identity()
            && self.aqua.is_identity()
            && self.blue.is_identity()
            && self.purple.is_identity()
            && self.magenta.is_identity()
    }
}

/// Computes the 8 band weights for any hue angle in degrees with exact partition of unity.
///
/// Returns an 8-element array summing identically to 1.0. At each band's reference knot,
/// that band's weight is 1.0 and all other weights are 0.0.
pub fn band_weights_deg(hue_deg: f32) -> [f32; BAND_COUNT] {
    let mut h = hue_deg % 360.0;
    if h < 0.0 {
        h += 360.0;
    }

    let mut weights = [0.0f32; BAND_COUNT];

    // Find the enclosing interval among the 8 knots:
    // Knots: theta_0, theta_1, ..., theta_7, and theta_8 = theta_0 + 360.0
    let theta_0 = BAND_HUES_DEG[0];
    let theta_last = BAND_HUES_DEG[BAND_COUNT - 1];

    if h < theta_0 {
        // Between band 7 (Magenta) and band 0 (Red)
        let span = theta_0 + 360.0 - theta_last;
        let t = (h + 360.0 - theta_last) / span;
        let w_next = 0.5 * (1.0 - (std::f32::consts::PI * t).cos());
        weights[0] = w_next;
        weights[BAND_COUNT - 1] = 1.0 - w_next;
        return weights;
    }

    if h >= theta_last {
        // Between band 7 (Magenta) and band 0 (Red)
        let span = theta_0 + 360.0 - theta_last;
        let t = (h - theta_last) / span;
        let w_next = 0.5 * (1.0 - (std::f32::consts::PI * t).cos());
        weights[0] = w_next;
        weights[BAND_COUNT - 1] = 1.0 - w_next;
        return weights;
    }

    for i in 0..(BAND_COUNT - 1) {
        let t0 = BAND_HUES_DEG[i];
        let t1 = BAND_HUES_DEG[i + 1];
        if h >= t0 && h <= t1 {
            let span = t1 - t0;
            let t = (h - t0) / span;
            let w_next = 0.5 * (1.0 - (std::f32::consts::PI * t).cos());
            weights[i + 1] = w_next;
            weights[i] = 1.0 - w_next;
            return weights;
        }
    }

    weights[0] = 1.0;
    weights
}

/// Size of the precompiled HSL circle lookup table (3600 entries = 0.1 degree resolution).
pub const HSL_LUT_SIZE: usize = 3600;

/// Precompiled table entry containing pre-multiplied 2D rotation/saturation and luminance scale/bias.
/// Exactly 16 bytes (128 bits), fitting 4 entries per 64-byte cache line and directly SIMD register-sized.
#[derive(Debug, Clone, Copy, Default)]
pub struct HslLutEntry {
    pub cos_sat: f32,
    pub sin_sat: f32,
    pub lum_scale: f32,
    pub lum_bias: f32,
}

/// Compiled HSL lookup table evaluated once per recipe change.
#[derive(Debug, Clone)]
pub struct CompiledHslTable {
    pub table: Box<[HslLutEntry; HSL_LUT_SIZE]>,
    pub global_hue_deg: f32,
    pub global_cos: f32,
    pub global_sin: f32,
    pub has_hsl: bool,
    pub has_hue: bool,
    pub is_identity: bool,
}

impl Default for CompiledHslTable {
    fn default() -> Self {
        let mut entries = Box::new([HslLutEntry::default(); HSL_LUT_SIZE]);
        for entry in entries.iter_mut() {
            entry.cos_sat = 1.0;
            entry.sin_sat = 0.0;
            entry.lum_scale = 1.0;
            entry.lum_bias = 0.0;
        }
        Self {
            table: entries,
            global_hue_deg: 0.0,
            global_cos: 1.0,
            global_sin: 0.0,
            has_hsl: false,
            has_hue: false,
            is_identity: true,
        }
    }
}

/// Fast, inlinable degree-accurate atan2 approximation on [0.0, 360.0).
///
/// Uses degree-scaled Remez minimax polynomial on [0, 1] with octant reduction.
/// Maximum error across the entire circle is < 0.0002 degrees (< 1/500th of an HSL LUT bin).
/// Avoids non-inlined libSystem atan2f calls during multi-million pixel passes.
#[inline(always)]
pub fn fast_atan2_deg(y: f32, x: f32) -> f32 {
    let abs_x = x.abs();
    let abs_y = y.abs();

    if abs_x < 1e-12 && abs_y < 1e-12 {
        return 0.0;
    }

    let (t, swap) = if abs_y <= abs_x {
        (abs_y / abs_x, false)
    } else {
        (abs_x / abs_y, true)
    };

    let t2 = t * t;
    let mut deg = t
        * (57.294477
            + t2 * (-19.057920
                + t2 * (11.089224 + t2 * (-6.671112 + t2 * (3.016813 - 0.671575 * t2)))));

    if swap {
        deg = 90.0 - deg;
    }

    if x < 0.0 {
        deg = 180.0 - deg;
    }
    if y < 0.0 {
        deg = 360.0 - deg;
    }
    if deg >= 360.0 {
        deg -= 360.0;
    }
    deg
}

/// Fast, inlinable LUT index computation on [0, 3600) for (y, x) coordinates.
///
/// Combines octant reduction and Remez minimax polynomial directly scaled to LUT size.
#[inline(always)]
pub fn fast_atan2_lut_idx(y: f32, x: f32) -> usize {
    let abs_x = x.abs();
    let abs_y = y.abs();

    let (t, swap) = if abs_y <= abs_x {
        (abs_y / abs_x, false)
    } else {
        (abs_x / abs_y, true)
    };

    let t2 = t * t;
    let mut idx_f = t * (572.78 + t2 * (-185.34 + 62.56 * t2));

    if swap {
        idx_f = 900.0 - idx_f;
    }

    if x < 0.0 {
        idx_f = 1800.0 - idx_f;
    }
    if y < 0.0 {
        idx_f = 3600.0 - idx_f;
    }
    if idx_f >= 3600.0 {
        idx_f = 0.0;
    }
    (idx_f as usize).min(HSL_LUT_SIZE - 1)
}

impl CompiledHslTable {
    /// Compiles `HslAdjustments` and global hue into a 3600-entry lookup table.
    pub fn from_recipe(hsl: Option<&HslAdjustments>, global_hue_deg: f32) -> Self {
        let has_hsl = hsl.is_some_and(|h| !h.is_identity());
        let has_hue = global_hue_deg != 0.0;

        if !has_hsl && !has_hue {
            return Self::default();
        }

        let (global_sin, global_cos) = global_hue_deg.to_radians().sin_cos();
        let bands = hsl.map(|h| h.bands()).unwrap_or_default();
        let mut entries = Box::new([HslLutEntry::default(); HSL_LUT_SIZE]);

        for (i, entry) in entries.iter_mut().enumerate() {
            let hue_deg = i as f32 * 0.1;
            let mut band_h = 0.0f32;
            let mut band_s = 0.0f32;
            let mut band_l = 0.0f32;

            if has_hsl {
                let w = band_weights_deg(hue_deg);
                for (k, band) in bands.iter().enumerate() {
                    let weight = w[k];
                    if weight > 0.0 {
                        band_h += weight * band.hue;
                        band_s += weight * (band.saturation / 100.0);
                        band_l += weight * (band.luminance / 100.0);
                    }
                }
            }

            let total_h_rad = (global_hue_deg + band_h).to_radians();
            let (sin, cos) = total_h_rad.sin_cos();
            let sat_scale = (1.0 + band_s).max(0.0);
            entry.cos_sat = cos * sat_scale;
            entry.sin_sat = sin * sat_scale;

            if band_l >= 0.0 {
                entry.lum_scale = 1.0 - band_l;
                entry.lum_bias = band_l;
            } else {
                entry.lum_scale = 1.0 + band_l;
                entry.lum_bias = 0.0;
            }
        }

        Self {
            table: entries,
            global_hue_deg,
            global_cos,
            global_sin,
            has_hsl,
            has_hue,
            is_identity: false,
        }
    }

    /// Evaluates HSL adjustments and global hue rotation with a precomputed chroma squared.
    #[inline(always)]
    pub fn apply_with_c_sq(&self, l: f32, a: f32, b: f32, c_sq: f32) -> (f32, f32, f32) {
        if self.is_identity {
            return (l, a, b);
        }

        const C_FADE_MIN_SQ: f32 = CHROMA_FADE_MIN * CHROMA_FADE_MIN;
        const C_FADE_MAX_SQ: f32 = CHROMA_FADE_MAX * CHROMA_FADE_MAX;

        // If only global hue is active: rotates at full strength at every chroma
        if !self.has_hsl {
            let final_a = a * self.global_cos - b * self.global_sin;
            let final_b = a * self.global_sin + b * self.global_cos;
            return (l, final_a, final_b);
        }

        // When HSL bands are active:
        // 1. Below CHROMA_FADE_MIN (C <= 0.005): band effect is 0; global hue applies at full strength.
        if c_sq <= C_FADE_MIN_SQ {
            if self.has_hue {
                let final_a = a * self.global_cos - b * self.global_sin;
                let final_b = a * self.global_sin + b * self.global_cos;
                (l, final_a, final_b)
            } else {
                (l, a, b)
            }
        } else if c_sq >= C_FADE_MAX_SQ {
            // 2. Above CHROMA_FADE_MAX (C >= 0.025): full strength.
            let lut_idx = fast_atan2_lut_idx(b, a);
            let entry = self.table[lut_idx.min(HSL_LUT_SIZE - 1)];

            let final_a = a * entry.cos_sat - b * entry.sin_sat;
            let final_b = a * entry.sin_sat + b * entry.cos_sat;
            let final_l = (l * entry.lum_scale + entry.lum_bias).clamp(0.0, 1.0);

            (final_l, final_a, final_b)
        } else {
            // 3. Low-chroma transition zone (0.005 < C < 0.025):
            // Global hue applies at full strength; band adjustments fade smoothly with f(C).
            let c = c_sq.sqrt();
            let fade = chroma_fade(c);

            let lut_idx = fast_atan2_lut_idx(b, a);
            let entry = self.table[lut_idx.min(HSL_LUT_SIZE - 1)];
            let a_global = a * self.global_cos - b * self.global_sin;
            let b_global = a * self.global_sin + b * self.global_cos;

            let full_a = a * entry.cos_sat - b * entry.sin_sat;
            let full_b = a * entry.sin_sat + b * entry.cos_sat;

            let final_a = a_global + fade * (full_a - a_global);
            let final_b = b_global + fade * (full_b - b_global);

            let full_l = l * entry.lum_scale + entry.lum_bias;
            let final_l = (l + fade * (full_l - l)).clamp(0.0, 1.0);

            (final_l, final_a, final_b)
        }
    }

    /// Evaluates HSL adjustments and global hue rotation on an Oklab (L, a, b) color in a single pass.
    ///
    /// Global hue rotation applies at full strength across every chroma (ED-9 preservation).
    /// Low-chroma fade applies exclusively to the 8 selective HSL bands, protecting neutral greys.
    #[inline(always)]
    pub fn apply(&self, l: f32, a: f32, b: f32) -> (f32, f32, f32) {
        let c_sq = a * a + b * b;
        self.apply_with_c_sq(l, a, b, c_sq)
    }

    /// Evaluates HSL adjustments and global hue rotation directly on linear sRGB (r, g, b).
    ///
    /// Skips Oklab reverse conversion if pixel is near-grey beneath chroma fade threshold.
    #[inline(always)]
    pub fn apply_linear_srgb(&self, r: f32, g: f32, b: f32) -> (f32, f32, f32) {
        if self.is_identity {
            return (r, g, b);
        }

        // Fast path for global hue only:
        if !self.has_hsl {
            return crate::media::edit::color::rotate_hue_oklch_sincos(
                r,
                g,
                b,
                self.global_cos,
                self.global_sin,
            );
        }

        let (l, a, b_val) = crate::media::edit::color::linear_srgb_to_oklab(r, g, b);
        let c_sq = a * a + b_val * b_val;
        const C_FADE_MIN_SQ: f32 = CHROMA_FADE_MIN * CHROMA_FADE_MIN;

        // If chroma <= 0.005 and no global hue, pixel is completely untouched:
        if c_sq <= C_FADE_MIN_SQ && !self.has_hue {
            return (r, g, b);
        }

        let (final_l, final_a, final_b) = self.apply_with_c_sq(l, a, b_val, c_sq);
        let (hr, hg, hb) =
            crate::media::edit::color::oklab_to_linear_srgb(final_l, final_a, final_b);
        (hr.max(0.0), hg.max(0.0), hb.max(0.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_band_is_centred_on_its_reference_colour() {
        for (i, (name, _srgb)) in REFERENCE_SRGB.iter().enumerate() {
            let knot_deg = BAND_HUES_DEG[i];
            let w = band_weights_deg(knot_deg);

            // Knot band must have weight exactly 1.0 (within float tolerance)
            assert!(
                (w[i] - 1.0).abs() < 1e-4,
                "Band {name} at knot {knot_deg}° expected weight 1.0, got {}",
                w[i]
            );

            // All other bands must have weight 0.0
            for (j, weight) in w.iter().enumerate() {
                if j != i {
                    assert!(
                        weight.abs() < 1e-4,
                        "Band {name} at knot {knot_deg}° expected other band {j} weight 0.0, got {weight}"
                    );
                }
            }
        }
    }

    #[test]
    fn band_weights_sum_to_one_at_every_hue() {
        // Test 3600 samples around the circle (every 0.1 degree)
        for i in 0..3600 {
            let hue = i as f32 * 0.1;
            let w = band_weights_deg(hue);
            let sum: f32 = w.iter().sum();
            assert!(
                (sum - 1.0).abs() < 1e-5,
                "Partition of unity failed at hue {hue}°: sum = {sum}"
            );
            for (band_idx, &weight) in w.iter().enumerate() {
                assert!(
                    (0.0..=1.00001).contains(&weight),
                    "Weight out of bounds [0, 1] at hue {hue}° for band {band_idx}: {weight}"
                );
            }
        }
    }

    #[test]
    fn moving_every_band_equally_matches_a_global_shift() {
        let global_delta_sat = 25.0;
        let global_delta_hue = 15.0;
        let global_delta_lum = -10.0;

        let uniform_band = HslBand {
            hue: global_delta_hue,
            saturation: global_delta_sat,
            luminance: global_delta_lum,
        };

        let hsl = HslAdjustments {
            red: uniform_band,
            orange: uniform_band,
            yellow: uniform_band,
            green: uniform_band,
            aqua: uniform_band,
            blue: uniform_band,
            purple: uniform_band,
            magenta: uniform_band,
        };

        let table = CompiledHslTable::from_recipe(Some(&hsl), 0.0);

        let expected_sat_scale = 1.0 + global_delta_sat / 100.0;
        let expected_cos_sat = global_delta_hue.to_radians().cos() * expected_sat_scale;
        let expected_sin_sat = global_delta_hue.to_radians().sin() * expected_sat_scale;
        let (expected_lum_scale, expected_lum_bias) = if global_delta_lum >= 0.0 {
            (1.0 - global_delta_lum / 100.0, global_delta_lum / 100.0)
        } else {
            (1.0 + global_delta_lum / 100.0, 0.0)
        };

        // Because weights sum to 1.0, moving every band equally must produce
        // the exact uniform shift across every single angle
        for i in 0..3600 {
            let entry = table.table[i];
            assert!(
                (entry.cos_sat - expected_cos_sat).abs() < 1e-4,
                "Cos sat mismatch at index {i}"
            );
            assert!(
                (entry.sin_sat - expected_sin_sat).abs() < 1e-4,
                "Sin sat mismatch at index {i}"
            );
            assert!(
                (entry.lum_scale - expected_lum_scale).abs() < 1e-4,
                "Lum scale mismatch at index {i}"
            );
            assert!(
                (entry.lum_bias - expected_lum_bias).abs() < 1e-4,
                "Lum bias mismatch at index {i}"
            );
        }
    }

    #[test]
    fn low_chroma_fade_protects_neutral_greys_and_near_greys() {
        let hsl = HslAdjustments {
            red: HslBand {
                hue: 45.0,
                saturation: 50.0,
                luminance: 30.0,
            },
            ..Default::default()
        };
        let table = CompiledHslTable::from_recipe(Some(&hsl), 0.0);

        // Exact grey (C = 0.0)
        let (l, a, b) = table.apply(0.5, 0.0, 0.0);
        assert_eq!((l, a, b), (0.5, 0.0, 0.0));

        // Near-grey beneath CHROMA_FADE_MIN (C = 0.003)
        let (l2, a2, b2) = table.apply(0.5, 0.003, 0.0);
        assert_eq!((l2, a2, b2), (0.5, 0.003, 0.0));

        // Full chroma (C = 0.15) should receive full effect
        let (l3, a3, _b3) = table.apply(0.5, 0.15, 0.0);
        assert!((l3 - 0.5).abs() > 0.01);
        assert!((a3 - 0.15).abs() > 0.01);
    }

    #[test]
    fn gamut_handling_prevents_negative_or_nan_escapes() {
        use crate::media::edit::color::oklab_to_linear_srgb;

        let extreme_hsl = HslAdjustments {
            red: HslBand {
                hue: 180.0,
                saturation: 100.0,
                luminance: 100.0,
            },
            blue: HslBand {
                hue: -180.0,
                saturation: -100.0,
                luminance: -100.0,
            },
            ..Default::default()
        };
        let table = CompiledHslTable::from_recipe(Some(&extreme_hsl), 0.0);

        for i in 0..360 {
            let rad = (i as f32).to_radians();
            let (a, b) = (0.25 * rad.cos(), 0.25 * rad.sin());
            let (l_out, a_out, b_out) = table.apply(0.7, a, b);

            assert!(!l_out.is_nan() && !l_out.is_infinite());
            assert!(!a_out.is_nan() && !a_out.is_infinite());
            assert!(!b_out.is_nan() && !b_out.is_infinite());
            assert!((0.0..=1.0).contains(&l_out));

            let (r, g, b_lin) = oklab_to_linear_srgb(l_out, a_out, b_out);
            assert!(!r.is_nan() && !g.is_nan() && !b_lin.is_nan());
        }
    }

    #[test]
    fn untouched_hsl_is_exact_identity() {
        let hsl = HslAdjustments::default();
        assert!(hsl.is_identity());
        let table = CompiledHslTable::from_recipe(Some(&hsl), 0.0);
        assert!(table.is_identity);

        let colors = [
            (0.5f32, 0.1f32, 0.1f32),
            (0.8, -0.2, 0.15),
            (0.2, 0.05, -0.1),
        ];

        for (l, a, b) in colors {
            let (l2, a2, b2) = table.apply(l, a, b);
            assert_eq!((l, a, b), (l2, a2, b2));
        }
    }

    #[test]
    fn fast_atan2_deg_matches_atan2_within_fraction_of_lut_bin() {
        for i in 0..3600 {
            let true_deg = i as f32 * 0.1;
            let rad = true_deg.to_radians();
            let (sin, cos) = rad.sin_cos();
            let approx_deg = fast_atan2_deg(sin, cos);
            let mut diff = (true_deg - approx_deg).abs();
            if diff > 180.0 {
                diff = (360.0 - diff).abs();
            }
            assert!(
                diff < 0.001,
                "fast_atan2_deg error {diff} deg at true angle {true_deg} deg"
            );
        }
    }

    #[test]
    fn the_global_hue_slider_rotates_low_chroma_colours_as_it_did_in_ed9() {
        use crate::media::edit::color::{oklab_to_linear_srgb, rotate_hue_oklch};

        // Pale color beneath CHROMA_FADE_MAX (C = 0.015)
        let (r_pale, g_pale, b_pale) = oklab_to_linear_srgb(0.6, 0.015, 0.0);
        let delta_hue = 45.0f32;

        // ED-9 formula:
        let (ed9_r, ed9_g, ed9_b) = rotate_hue_oklch(r_pale, g_pale, b_pale, delta_hue);

        // ED-11 formula via CompiledHslTable:
        let table = CompiledHslTable::from_recipe(None, delta_hue);
        let (ed11_r, ed11_g, ed11_b) = table.apply_linear_srgb(r_pale, g_pale, b_pale);

        assert!(
            (ed9_r - ed11_r).abs() < 1e-5,
            "Red channel mismatch at low chroma: ed9 {ed9_r} vs ed11 {ed11_r}"
        );
        assert!(
            (ed9_g - ed11_g).abs() < 1e-5,
            "Green channel mismatch at low chroma: ed9 {ed9_g} vs ed11 {ed11_g}"
        );
        assert!(
            (ed9_b - ed11_b).abs() < 1e-5,
            "Blue channel mismatch at low chroma: ed9 {ed9_b} vs ed11 {ed11_b}"
        );
    }

    #[test]
    fn red_hue_shift_only_affects_red_band_with_smooth_boundary_falloff() {
        let hsl = HslAdjustments {
            red: HslBand {
                hue: 30.0,
                saturation: 0.0,
                luminance: 0.0,
            },
            ..Default::default()
        };
        let table = CompiledHslTable::from_recipe(Some(&hsl), 0.0);

        let l = 0.7f32;
        let c = 0.1f32; // Ordinary chroma, well above CHROMA_FADE_MAX (0.025)

        // 1. Pixel at the Red centre rotates by 30° within LUT bin tolerance (0.1° resolution)
        let knot_red = BAND_HUES_DEG[0]; // 29.23°
        let (a_red, b_red) = (
            c * knot_red.to_radians().cos(),
            c * knot_red.to_radians().sin(),
        );
        let (l_red_out, a_red_out, b_red_out) = table.apply(l, a_red, b_red);
        assert!((l_red_out - l).abs() < 1e-4);
        let out_hue_red = fast_atan2_deg(b_red_out, a_red_out);
        let mut delta_red = out_hue_red - knot_red;
        if delta_red < -180.0 {
            delta_red += 360.0;
        } else if delta_red > 180.0 {
            delta_red -= 360.0;
        }
        assert!(
            (delta_red - 30.0).abs() < 0.15,
            "Red centre expected ~30.0° rotation, got {delta_red}°"
        );

        // 2. Pixels at Green, Aqua, Blue, Purple centres are unchanged (tolerance 1e-6)
        // Green=3, Aqua=4, Blue=5, Purple=6
        for &idx in &[3, 4, 5, 6] {
            let knot = BAND_HUES_DEG[idx];
            let (a, b) = (c * knot.to_radians().cos(), c * knot.to_radians().sin());
            let (l_out, a_out, b_out) = table.apply(l, a, b);
            assert!(
                (l_out - l).abs() < 1e-6,
                "Luminance changed at band {idx} ({knot}°): {l_out} vs {l}"
            );
            assert!(
                (a_out - a).abs() < 1e-6,
                "a changed at band {idx} ({knot}°): {a_out} vs {a}"
            );
            assert!(
                (b_out - b).abs() < 1e-6,
                "b changed at band {idx} ({knot}°): {b_out} vs {b}"
            );
        }

        // 3. Sweeping hue from Red centre to Orange centre in >= 50 steps
        // The applied rotation falls monotonically from 30° to 0° with bounded step size.
        let knot_orange = BAND_HUES_DEG[1]; // 52.78°
        let steps = 60;
        let mut prev_rot = 30.0f32;
        for s in 0..=steps {
            let t = s as f32 / steps as f32;
            let hue = knot_red + t * (knot_orange - knot_red);
            let (a, b) = (c * hue.to_radians().cos(), c * hue.to_radians().sin());
            let (_, a_out, b_out) = table.apply(l, a, b);
            let out_hue = fast_atan2_deg(b_out, a_out);
            let mut rot = out_hue - hue;
            if rot < -180.0 {
                rot += 360.0;
            } else if rot > 180.0 {
                rot -= 360.0;
            }

            // Monotonic falloff (with tiny 0.05° allowance for 0.1° LUT quantization steps)
            assert!(
                rot <= prev_rot + 0.05,
                "Non-monotonic rotation falloff at step {s} (hue {hue}°): prev {prev_rot}°, current {rot}°"
            );
            // Bounded step difference: with 60 steps over 30°, max derivative of raised-cosine is ~1.5x average
            let step_diff = (prev_rot - rot).abs();
            assert!(
                step_diff <= 1.5,
                "Step difference too large at step {s}: {step_diff}°"
            );
            prev_rot = rot;
        }
        assert!(
            prev_rot.abs() < 0.1,
            "Expected ~0° rotation at Orange knot, got {prev_rot}°"
        );
    }

    #[test]
    fn saturation_and_luminance_shifts_within_a_band_preserve_other_hues() {
        let hsl = HslAdjustments {
            blue: HslBand {
                hue: 0.0,
                saturation: 50.0,
                luminance: -30.0,
            },
            ..Default::default()
        };
        let table = CompiledHslTable::from_recipe(Some(&hsl), 0.0);

        let l = 0.7f32;
        let c = 0.1f32;

        // 1. Blue-centre pixel gains chroma and loses L
        let knot_blue = BAND_HUES_DEG[5]; // 264.05°
        let (a_blue, b_blue) = (
            c * knot_blue.to_radians().cos(),
            c * knot_blue.to_radians().sin(),
        );
        let (l_out, a_out, b_out) = table.apply(l, a_blue, b_blue);
        let c_out = (a_out * a_out + b_out * b_out).sqrt();

        // Saturation +50% -> chroma increases by ~50%
        assert!(
            c_out > c + 0.03,
            "Expected chroma gain at Blue knot: original {c}, got {c_out}"
        );
        // Luminance -30% -> L decreases significantly
        assert!(
            l_out < l - 0.1,
            "Expected luminance loss at Blue knot: original {l}, got {l_out}"
        );

        // 2. Red, Yellow and Green-centre pixels are unchanged (tolerance 1e-6)
        // Red=0, Yellow=2, Green=3
        for &idx in &[0, 2, 3] {
            let knot = BAND_HUES_DEG[idx];
            let (a, b) = (c * knot.to_radians().cos(), c * knot.to_radians().sin());
            let (l_out, a_out, b_out) = table.apply(l, a, b);
            assert!(
                (l_out - l).abs() < 1e-6,
                "Luminance changed at band {idx} ({knot}°): {l_out} vs {l}"
            );
            assert!(
                (a_out - a).abs() < 1e-6,
                "a changed at band {idx} ({knot}°): {a_out} vs {a}"
            );
            assert!(
                (b_out - b).abs() < 1e-6,
                "b changed at band {idx} ({knot}°): {b_out} vs {b}"
            );
        }
    }
}
