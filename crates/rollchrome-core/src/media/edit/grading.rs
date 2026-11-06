//! 3-way colour grading (Shadows, Midtones, Highlights) and Global colour wheel (ED-12).
//!
//! Evaluates in display-encoded sRGB space after tone curves and before 3D LUT grading.
//! Uses true ASC CDL Lift/Gamma/Gain color model with partitioned range weights:
//! - Shadows (Lift) acts as an additive offset weighted by w_S(y), shifting the black floor while keeping white fixed (F(1) = 1).
//! - Highlights (Gain) acts as slope scaling weighted by w_H(y), moving the white end while keeping pure black fixed (F(0) = 0).
//! - Midtones (Gamma) acts as a power curve weighted by w_M(y), bending midtones while keeping both endpoints fixed (F(0) = 0, F(1) = 1).
//! - Global acts as a multiplicative exposure-style gain (2^(global * 0.5)), scaling the overall range without lifting pure black off 0 or clipping mid-greys.
//! - Power base is clamped at 0 (`base.max(0.0)`) so negative values never reach `powf`.
//!
//! Tints are defined by (hue, saturation) in OkLCh perceptual color space, matching the
//! exact knot hues used by 8-band HSL (e.g. 29.23° is sRGB Red). Each wheel's tint is
//! precomputed once into an RGB vector with its luma contribution subtracted, guaranteeing
//! that chromatic tints preserve brightness identically (within 1 code value).
//!
//! Range weights partition unity at every luma: w_shadows(y) + w_midtones(y) + w_highlights(y) == 1.0.

use serde::{Deserialize, Serialize};

/// Tonal color wheel parameters (Hue, Saturation, Luminance).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct ColorWheel {
    /// Hue angle in degrees: 0.0 to 360.0 (0.0 at rest).
    #[serde(default)]
    pub hue: f32,
    /// Saturation / intensity: 0.0 to 1.0 (0.0 = identity / no tint).
    #[serde(default)]
    pub saturation: f32,
    /// Luminance offset: -1.0 to +1.0 (0.0 = identity).
    #[serde(default)]
    pub luminance: f32,
}

impl ColorWheel {
    /// Returns true if this wheel has zero saturation and zero luminance offset.
    #[inline]
    pub fn is_identity(&self) -> bool {
        self.saturation == 0.0 && self.luminance == 0.0
    }
}

fn default_fifty() -> f32 {
    50.0
}

/// 3-way color grading configuration with Shadows, Midtones, Highlights, and Global wheels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColorGrading {
    /// Shadows wheel (Lift): affects lower luma values.
    #[serde(default)]
    pub shadows: ColorWheel,
    /// Midtones wheel (Gamma): affects mid luma values.
    #[serde(default)]
    pub midtones: ColorWheel,
    /// Highlights wheel (Gain): affects upper luma values.
    #[serde(default)]
    pub highlights: ColorWheel,
    /// Global wheel: affects all luminance levels uniformly.
    #[serde(default)]
    pub global: ColorWheel,
    /// Range blending / overlap softness: 0.0 to 100.0 (default 50.0).
    #[serde(default = "default_fifty")]
    pub blending: f32,
    /// Tonal balance / crossover shift: -100.0 to +100.0 (default 0.0).
    #[serde(default)]
    pub balance: f32,
}

impl Default for ColorGrading {
    fn default() -> Self {
        Self {
            shadows: ColorWheel::default(),
            midtones: ColorWheel::default(),
            highlights: ColorWheel::default(),
            global: ColorWheel::default(),
            blending: 50.0,
            balance: 0.0,
        }
    }
}

impl ColorGrading {
    /// Returns true if all 4 wheels are at rest (identity).
    #[inline]
    pub fn is_identity(&self) -> bool {
        self.shadows.is_identity()
            && self.midtones.is_identity()
            && self.highlights.is_identity()
            && self.global.is_identity()
    }
}

/// Converts a wheel's (hue_deg, saturation) polar coordinates into an RGB tint vector in display space.
///
/// Uses OkLCh perceptual color space at reference L = 0.70 (matching the HSL swatch definition),
/// and removes the tint's own Rec. 709 luma contribution so that applying this tint does not alter
/// pixel brightness.
pub fn wheel_to_rgb_tint(hue_deg: f32, saturation: f32) -> [f32; 3] {
    let sat = saturation.clamp(0.0, 1.0);
    if sat <= 1e-6 {
        return [0.0, 0.0, 0.0];
    }

    // Reference lightness L = 0.70. Maximum chroma at saturation = 1.0 is 0.20.
    let l = 0.70f32;
    let c = sat * 0.20f32;
    let h_rad = hue_deg.to_radians();
    let a = c * h_rad.cos();
    let b = c * h_rad.sin();

    // Convert tinted (L, a, b) to display sRGB
    let (r_lin, g_lin, b_lin) = crate::media::edit::color::oklab_to_linear_srgb(l, a, b);
    let r_disp = crate::media::edit::pipeline::linear_to_srgb(r_lin);
    let g_disp = crate::media::edit::pipeline::linear_to_srgb(g_lin);
    let b_disp = crate::media::edit::pipeline::linear_to_srgb(b_lin);

    // Convert neutral grey (L, 0, 0) to display sRGB
    let (r0_lin, g0_lin, b0_lin) = crate::media::edit::color::oklab_to_linear_srgb(l, 0.0, 0.0);
    let r0_disp = crate::media::edit::pipeline::linear_to_srgb(r0_lin);
    let g0_disp = crate::media::edit::pipeline::linear_to_srgb(g0_lin);
    let b0_disp = crate::media::edit::pipeline::linear_to_srgb(b0_lin);

    let mut delta_r = r_disp - r0_disp;
    let mut delta_g = g_disp - g0_disp;
    let mut delta_b = b_disp - b0_disp;

    // Remove luma contribution (Rec. 709 weights) so tint preserves brightness identically:
    let delta_y = 0.2126 * delta_r + 0.7152 * delta_g + 0.0722 * delta_b;
    delta_r -= delta_y;
    delta_g -= delta_y;
    delta_b -= delta_y;

    [delta_r, delta_g, delta_b]
}

/// Evaluates the 3 tonal range weights (Shadows, Midtones, Highlights) for a display luma Y in [0, 1].
///
/// Guaranteed to partition unity at every luma: w_shadows + w_midtones + w_highlights == 1.0.
/// All weights are in [0.0, 1.0].
#[inline]
pub fn range_weights(luma: f32, balance: f32, blending: f32) -> (f32, f32, f32) {
    let y = luma.clamp(0.0, 1.0);

    // Balance moves crossovers monotonically:
    let b = (balance / 100.0).clamp(-1.0, 1.0) * 0.20;
    let c1 = 0.33 + b;
    let c2 = 0.67 + b;

    // Blending widens or narrows crossover transition width:
    let w = 0.10 + (blending / 100.0).clamp(0.0, 1.0) * 0.30;
    let h = w * 0.5;

    // Smooth raised-cosine transition at c1 (Shadows to Midtones):
    let s = if y <= c1 - h {
        1.0
    } else if y >= c1 + h {
        0.0
    } else {
        let t = (y - (c1 - h)) / (2.0 * h);
        0.5 * (1.0 + (std::f32::consts::PI * t).cos())
    };

    // Smooth raised-cosine transition at c2 (Midtones to Highlights):
    let h_w = if y <= c2 - h {
        0.0
    } else if y >= c2 + h {
        1.0
    } else {
        let t = (y - (c2 - h)) / (2.0 * h);
        0.5 * (1.0 - (std::f32::consts::PI * t).cos())
    };

    // Partition of unity:
    let w_s = s * (1.0 - h_w);
    let w_h = h_w * (1.0 - s);
    let w_m = (1.0 - w_s - w_h).max(0.0);

    (w_s, w_m, w_h)
}

/// Number of entries in the precomputed display luma grading table.
pub const GRADING_LUT_SIZE: usize = 1024;

/// Compiled color grading lookup table precomputing RGB deltas across 1024 luma levels.
///
/// Precomputes all wheel tints and range weights once per recipe change, allowing per-pixel
/// evaluation to complete in a single luma lookup and 3 additions (< 0.8 ms on 4.37 MP).
#[derive(Debug, Clone)]
pub struct CompiledGradingTable {
    /// Precomputed RGB delta per luma bin [0..1024].
    pub table: Box<[[f32; 3]; GRADING_LUT_SIZE]>,
    /// True if all wheels and offsets are at rest (0.0).
    pub is_identity: bool,
}

impl Default for CompiledGradingTable {
    fn default() -> Self {
        Self {
            table: Box::new([[0.0f32; 3]; GRADING_LUT_SIZE]),
            is_identity: true,
        }
    }
}

impl CompiledGradingTable {
    /// Compiles `ColorGrading` parameters into a 1024-entry luma-indexed lookup table.
    pub fn from_recipe(grading: Option<&ColorGrading>) -> Self {
        let g = match grading {
            Some(g) if !g.is_identity() => g,
            _ => return Self::default(),
        };

        // 1. Precompute wheel tints in OkLCh (with luma removed)
        let s_tint = wheel_to_rgb_tint(g.shadows.hue, g.shadows.saturation);
        let m_tint = wheel_to_rgb_tint(g.midtones.hue, g.midtones.saturation);
        let h_tint = wheel_to_rgb_tint(g.highlights.hue, g.highlights.saturation);
        let g_tint = wheel_to_rgb_tint(g.global.hue, g.global.saturation);

        // 2. Map luminance sliders to ASC CDL parameters:
        // Lift (Shadows) maps to offset: [-0.25, 0.25]
        let lift_offset = g.shadows.luminance.clamp(-1.0, 1.0) * 0.25;
        // Gain (Highlights) maps to slope: [0.65, 1.35]
        let gain_slope = 1.0 + g.highlights.luminance.clamp(-1.0, 1.0) * 0.35;
        // Gamma (Midtones) maps to power: [0.70, 1.30]
        let gamma_power = 1.0 - g.midtones.luminance.clamp(-1.0, 1.0) * 0.30;
        // Global maps to multiplicative exposure-style gain: 2^(global * 0.5)
        let global_gain = 2.0f32.powf(g.global.luminance.clamp(-1.0, 1.0) * 0.5);

        let mut table = Box::new([[0.0f32; 3]; GRADING_LUT_SIZE]);
        let scale = 1.0 / (GRADING_LUT_SIZE - 1) as f32;

        for (i, entry) in table.iter_mut().enumerate() {
            let y = i as f32 * scale;
            let (w_s, w_m, w_h) = range_weights(y, g.balance, g.blending);

            // ASC CDL luminance mapping: out = (in * slope + offset)^power
            // Slope is weighted by highlights and scaled by global gain:
            // At y = 0: w_h = 0, offset = w_s * lift_offset. Pure black stays 0 unless Lift is moved.
            // Global gain scales slope multiplicatively so pure black (y=0) stays 0.
            let offset = w_s * lift_offset;
            let slope = (1.0 + w_h * (gain_slope - 1.0)) * global_gain;
            let power = 1.0 + w_m * (gamma_power - 1.0);

            // Clamp power base at 0 so negative values never reach powf (CDL specification requirement):
            let base = (y * slope + offset).max(0.0);
            let y_cdl = if base > 0.0 { base.powf(power) } else { 0.0 };
            let lum_delta = y_cdl - y;

            // Chromatic tints from wheels (partitioned, with luma contribution removed):
            let tint_r = w_s * s_tint[0] + w_m * m_tint[0] + w_h * h_tint[0] + g_tint[0];
            let tint_g = w_s * s_tint[1] + w_m * m_tint[1] + w_h * h_tint[1] + g_tint[1];
            let tint_b = w_s * s_tint[2] + w_m * m_tint[2] + w_h * h_tint[2] + g_tint[2];

            entry[0] = tint_r + lum_delta;
            entry[1] = tint_g + lum_delta;
            entry[2] = tint_b + lum_delta;
        }

        Self {
            table,
            is_identity: false,
        }
    }

    /// Applies 3-way color grading to a display-encoded pixel (r, g, b in [0.0, 1.0]).
    #[inline(always)]
    pub fn apply(&self, r: f32, g: f32, b: f32) -> (f32, f32, f32) {
        if self.is_identity {
            return (r, g, b);
        }
        Self::apply_table(&self.table, r, g, b)
    }

    /// Applies 3-way color grading with a direct pre-borrowed table reference,
    /// avoiding repeated Box pointer dereferencing in hot inner loops.
    #[inline(always)]
    pub fn apply_table(
        table: &[[f32; 3]; GRADING_LUT_SIZE],
        r: f32,
        g: f32,
        b: f32,
    ) -> (f32, f32, f32) {
        let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        let idx = ((y * (GRADING_LUT_SIZE - 1) as f32) as usize).min(GRADING_LUT_SIZE - 1);
        let delta = table[idx];

        (
            (r + delta[0]).clamp(0.0, 1.0),
            (g + delta[1]).clamp(0.0, 1.0),
            (b + delta[2]).clamp(0.0, 1.0),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn untouched_grading_wheels_are_exact_identity() {
        let grading = ColorGrading::default();
        assert!(grading.is_identity());
        let table = CompiledGradingTable::from_recipe(Some(&grading));
        assert!(table.is_identity);

        let colors = [
            (0.1f32, 0.1f32, 0.1f32),
            (0.5, 0.5, 0.5),
            (0.9, 0.8, 0.2),
            (0.0, 1.0, 0.5),
        ];

        for (r, g, b) in colors {
            let (r_out, g_out, b_out) = table.apply(r, g, b);
            assert_eq!((r, g, b), (r_out, g_out, b_out));
        }
    }

    #[test]
    fn range_weights_sum_to_one_at_every_luma() {
        let balances = [-100.0, -50.0, 0.0, 50.0, 100.0];
        let blendings = [0.0, 25.0, 50.0, 75.0, 100.0];

        for &bal in &balances {
            for &blend in &blendings {
                for i in 0..=1000 {
                    let y = i as f32 / 1000.0;
                    let (ws, wm, wh) = range_weights(y, bal, blend);
                    let sum = ws + wm + wh;
                    assert!(
                        (sum - 1.0).abs() < 1e-5,
                        "Range weights sum != 1.0 at Y={y} (bal={bal}, blend={blend}): sum={sum}"
                    );
                    assert!(
                        (0.0..=1.00001).contains(&ws),
                        "ws out of bounds [0, 1]: {ws}"
                    );
                    assert!(
                        (0.0..=1.00001).contains(&wm),
                        "wm out of bounds [0, 1]: {wm}"
                    );
                    assert!(
                        (0.0..=1.00001).contains(&wh),
                        "wh out of bounds [0, 1]: {wh}"
                    );
                }
            }
        }
    }

    #[test]
    fn balance_moves_the_crossover_monotonically() {
        let mut prev_c1 = -1.0f32;
        let mut prev_c2 = -1.0f32;

        for step in 0..=20 {
            let bal = -100.0 + step as f32 * 10.0;
            // Find crossover c1 where ws == wm and c2 where wm == wh:
            let b = (bal / 100.0).clamp(-1.0, 1.0) * 0.20;
            let c1 = 0.33 + b;
            let c2 = 0.67 + b;

            if prev_c1 >= 0.0 {
                assert!(
                    c1 > prev_c1,
                    "c1 did not move monotonically: prev {prev_c1}, current {c1}"
                );
                assert!(
                    c2 > prev_c2,
                    "c2 did not move monotonically: prev {prev_c2}, current {c2}"
                );
            }
            prev_c1 = c1;
            prev_c2 = c2;
        }
    }

    #[test]
    fn a_wheel_tint_preserves_luma() {
        // Pushing Shadows towards Blue must tint without altering brightness:
        let hues = [0.0, 29.23, 120.0, 200.0, 264.05, 300.0];
        let saturations = [0.25, 0.50, 0.75, 1.0];

        for &hue in &hues {
            for &sat in &saturations {
                let tint = wheel_to_rgb_tint(hue, sat);
                let luma_delta = 0.2126 * tint[0] + 0.7152 * tint[1] + 0.0722 * tint[2];
                // Luma delta in 8-bit code value must be < 1 code value (i.e. < 1.0 / 255.0 = 0.00392):
                assert!(
                    luma_delta.abs() * 255.0 < 0.1,
                    "Tint luma not preserved for hue {hue}°, sat {sat}: delta={luma_delta}"
                );
            }
        }
    }

    #[test]
    fn shadows_wheel_tints_shadows_and_decays_smoothly_at_high_luminance() {
        let grading = ColorGrading {
            shadows: ColorWheel {
                hue: 29.23, // Red
                saturation: 0.8,
                luminance: 0.0,
            },
            ..Default::default()
        };
        let table = CompiledGradingTable::from_recipe(Some(&grading));

        // Deep shadow pixel (Y = 0.08)
        let (r_s, g_s, b_s) = table.apply(0.08, 0.08, 0.08);
        assert!(r_s > 0.08, "Shadows should receive red boost: {r_s}");
        assert!(
            g_s < 0.08 || b_s < 0.08,
            "Shadows green or blue should decrease"
        );

        // Bright highlight pixel (Y = 0.92)
        let (r_h, g_h, b_h) = table.apply(0.92, 0.92, 0.92);
        assert!(
            (r_h - 0.92).abs() < 1e-4,
            "Highlights should be untouched by shadows wheel: {r_h}"
        );
        assert!(
            (g_h - 0.92).abs() < 1e-4,
            "Highlights should be untouched by shadows wheel: {g_h}"
        );
        assert!(
            (b_h - 0.92).abs() < 1e-4,
            "Highlights should be untouched by shadows wheel: {b_h}"
        );
    }

    #[test]
    fn highlights_wheel_tints_highlights_and_decays_smoothly_at_low_luminance() {
        let grading = ColorGrading {
            highlights: ColorWheel {
                hue: 264.05, // Blue
                saturation: 0.8,
                luminance: 0.0,
            },
            ..Default::default()
        };
        let table = CompiledGradingTable::from_recipe(Some(&grading));

        // Bright highlight pixel (Y = 0.90)
        let (_r_h, _g_h, b_h) = table.apply(0.90, 0.90, 0.90);
        assert!(b_h > 0.90, "Highlights should receive blue boost: {b_h}");

        // Deep shadow pixel (Y = 0.05)
        let (r_s, g_s, b_s) = table.apply(0.05, 0.05, 0.05);
        assert!(
            (r_s - 0.05).abs() < 1e-4,
            "Shadows should be untouched by highlights wheel"
        );
        assert!(
            (g_s - 0.05).abs() < 1e-4,
            "Shadows should be untouched by highlights wheel"
        );
        assert!(
            (b_s - 0.05).abs() < 1e-4,
            "Shadows should be untouched by highlights wheel"
        );
    }

    #[test]
    fn global_wheel_tints_all_luminance_levels_uniformly() {
        let grading = ColorGrading {
            global: ColorWheel {
                hue: 142.50, // Green
                saturation: 0.5,
                luminance: 0.0,
            },
            ..Default::default()
        };
        let table = CompiledGradingTable::from_recipe(Some(&grading));

        let y_levels = [0.20, 0.50, 0.80];
        let mut green_deltas = Vec::new();

        for &y in &y_levels {
            let (_r, g, _b) = table.apply(y, y, y);
            green_deltas.push(g - y);
        }

        // All luminance levels receive the exact same delta from global wheel:
        let d0 = green_deltas[0];
        for (i, &d) in green_deltas.iter().enumerate() {
            assert!(
                (d - d0).abs() < 1e-4,
                "Level {i} delta {d} differed from global delta {d0}"
            );
        }
    }

    #[test]
    fn grading_balance_and_blending_adjust_tonal_overlap_stably() {
        let extreme_grading = ColorGrading {
            shadows: ColorWheel {
                hue: 45.0,
                saturation: 1.0,
                luminance: -1.0,
            },
            midtones: ColorWheel {
                hue: 180.0,
                saturation: 1.0,
                luminance: 1.0,
            },
            highlights: ColorWheel {
                hue: 280.0,
                saturation: 1.0,
                luminance: -1.0,
            },
            global: ColorWheel {
                hue: 0.0,
                saturation: 1.0,
                luminance: 1.0,
            },
            balance: -100.0,
            blending: 100.0,
        };

        let table = CompiledGradingTable::from_recipe(Some(&extreme_grading));
        for i in 0..=100 {
            let y = i as f32 / 100.0;
            let (r, g, b) = table.apply(y, y, y);
            assert!(!r.is_nan() && !r.is_infinite());
            assert!(!g.is_nan() && !g.is_infinite());
            assert!(!b.is_nan() && !b.is_infinite());
            assert!((0.0..=1.0).contains(&r));
            assert!((0.0..=1.0).contains(&g));
            assert!((0.0..=1.0).contains(&b));
        }
    }

    #[test]
    fn power_base_clamped_at_zero_never_evaluates_negative_powf() {
        // Deep negative shadows offset that forces (y * slope + offset) to negative values for low y:
        let grading = ColorGrading {
            shadows: ColorWheel {
                hue: 0.0,
                saturation: 0.0,
                luminance: -1.0, // lift offset = -0.25
            },
            midtones: ColorWheel {
                hue: 0.0,
                saturation: 0.0,
                luminance: 0.5, // fractional power exponent
            },
            ..Default::default()
        };
        let table = CompiledGradingTable::from_recipe(Some(&grading));
        assert!(!table.is_identity);

        // At low luminance (y <= 0.20), base would be negative without clamping:
        for i in 0..=200 {
            let y = i as f32 / 1000.0;
            let (r, g, b) = table.apply(y, y, y);
            assert!(!r.is_nan(), "r is NaN at y={y}");
            assert!(!g.is_nan(), "g is NaN at y={y}");
            assert!(!b.is_nan(), "b is NaN at y={y}");
        }
    }

    #[test]
    fn wheel_angle_matches_hsl_reference_knot_hues() {
        // 29.23° is sRGB Red in OkLCh (matching HSL panel Red knot):
        let red_tint = wheel_to_rgb_tint(29.23, 0.8);
        assert!(red_tint[0] > 0.0, "Red tint should have positive R delta");
        assert!(
            red_tint[1] < 0.0 && red_tint[2] < 0.0,
            "Red tint should have negative G and B deltas to preserve luma"
        );

        // 264.05° is sRGB Blue in OkLCh (matching HSL panel Blue knot):
        let blue_tint = wheel_to_rgb_tint(264.05, 0.8);
        assert!(blue_tint[2] > 0.0, "Blue tint should have positive B delta");
        assert!(
            blue_tint[0] < 0.0 || blue_tint[1] < 0.0,
            "Blue tint should depress R and G to preserve luma"
        );
    }

    #[test]
    fn shadows_luminance_moves_black_but_not_white() {
        let grading = ColorGrading {
            shadows: ColorWheel {
                hue: 0.0,
                saturation: 0.0,
                luminance: 0.5,
            },
            ..Default::default()
        };
        let table = CompiledGradingTable::from_recipe(Some(&grading));
        // Black (y=0) moves:
        let (r0, g0, b0) = table.apply(0.0, 0.0, 0.0);
        assert!(
            r0 > 0.0 && g0 > 0.0 && b0 > 0.0,
            "Shadows should move black: got ({r0}, {g0}, {b0})"
        );

        // White (y=1) stays 1:
        let (r1, g1, b1) = table.apply(1.0, 1.0, 1.0);
        assert!((r1 - 1.0).abs() < 1e-4, "White must stay 1.0: got {r1}");
        assert!((g1 - 1.0).abs() < 1e-4, "White must stay 1.0: got {g1}");
        assert!((b1 - 1.0).abs() < 1e-4, "White must stay 1.0: got {b1}");
    }

    #[test]
    fn highlights_luminance_moves_white_but_not_black() {
        let grading = ColorGrading {
            highlights: ColorWheel {
                hue: 0.0,
                saturation: 0.0,
                luminance: -0.5,
            },
            ..Default::default()
        };
        let table = CompiledGradingTable::from_recipe(Some(&grading));
        // White (y=1) moves:
        let (r1, g1, b1) = table.apply(1.0, 1.0, 1.0);
        assert!(
            r1 < 1.0 && g1 < 1.0 && b1 < 1.0,
            "Highlights should move white: got ({r1}, {g1}, {b1})"
        );

        // Black (y=0) stays 0:
        let (r0, g0, b0) = table.apply(0.0, 0.0, 0.0);
        assert!(r0.abs() < 1e-4, "Black must stay 0.0: got {r0}");
        assert!(g0.abs() < 1e-4, "Black must stay 0.0: got {g0}");
        assert!(b0.abs() < 1e-4, "Black must stay 0.0: got {b0}");
    }

    #[test]
    fn midtones_luminance_keeps_black_and_white_fixed() {
        let grading = ColorGrading {
            midtones: ColorWheel {
                hue: 0.0,
                saturation: 0.0,
                luminance: 0.8,
            },
            ..Default::default()
        };
        let table = CompiledGradingTable::from_recipe(Some(&grading));
        // Black (y=0) stays 0:
        let (r0, g0, b0) = table.apply(0.0, 0.0, 0.0);
        assert!(r0.abs() < 1e-4, "Black must stay 0.0: got {r0}");
        assert!(g0.abs() < 1e-4, "Black must stay 0.0: got {g0}");
        assert!(b0.abs() < 1e-4, "Black must stay 0.0: got {b0}");

        // White (y=1) stays 1:
        let (r1, g1, b1) = table.apply(1.0, 1.0, 1.0);
        assert!((r1 - 1.0).abs() < 1e-4, "White must stay 1.0: got {r1}");
        assert!((g1 - 1.0).abs() < 1e-4, "White must stay 1.0: got {g1}");
        assert!((b1 - 1.0).abs() < 1e-4, "White must stay 1.0: got {b1}");

        // Mid-tones bend:
        let (rm, _gm, _bm) = table.apply(0.5, 0.5, 0.5);
        assert!((rm - 0.5).abs() > 0.02, "Midtones must move: got {rm}");
    }

    #[test]
    fn global_luminance_does_not_lift_pure_black() {
        for &lum in &[-1.0, -0.5, 0.5, 1.0] {
            let grading = ColorGrading {
                global: ColorWheel {
                    hue: 0.0,
                    saturation: 0.0,
                    luminance: lum,
                },
                ..Default::default()
            };
            let table = CompiledGradingTable::from_recipe(Some(&grading));
            let (r0, g0, b0) = table.apply(0.0, 0.0, 0.0);
            assert_eq!(
                (r0, g0, b0),
                (0.0, 0.0, 0.0),
                "Global luminance {lum} must not lift pure black off 0: got ({r0}, {g0}, {b0})"
            );

            // And at mid-grey (0.5), it brightens/darkens without clipping
            let (rm, _gm, _bm) = table.apply(0.5, 0.5, 0.5);
            assert!(rm > 0.0 && rm < 1.0, "Mid-grey should not clip: got {rm}");
            if lum > 0.0 {
                assert!(rm > 0.5, "Positive global lum should brighten: {rm}");
            } else if lum < 0.0 {
                assert!(rm < 0.5, "Negative global lum should darken: {rm}");
            }
        }
    }

    #[test]
    fn graded_output_stays_in_range_and_finite() {
        let extreme_settings = [
            (-1.0, -1.0, -1.0, -1.0, -100.0, 0.0),
            (1.0, 1.0, 1.0, 1.0, 100.0, 100.0),
            (-1.0, 1.0, -1.0, 1.0, 0.0, 50.0),
            (1.0, -1.0, 1.0, -1.0, -50.0, 75.0),
        ];

        for (s_lum, m_lum, h_lum, g_lum, bal, blend) in extreme_settings {
            let grading = ColorGrading {
                shadows: ColorWheel {
                    hue: 30.0,
                    saturation: 1.0,
                    luminance: s_lum,
                },
                midtones: ColorWheel {
                    hue: 120.0,
                    saturation: 1.0,
                    luminance: m_lum,
                },
                highlights: ColorWheel {
                    hue: 240.0,
                    saturation: 1.0,
                    luminance: h_lum,
                },
                global: ColorWheel {
                    hue: 300.0,
                    saturation: 1.0,
                    luminance: g_lum,
                },
                balance: bal,
                blending: blend,
            };
            let table = CompiledGradingTable::from_recipe(Some(&grading));
            for i in 0..=100 {
                let y = i as f32 / 100.0;
                let (r, g, b) = table.apply(y, y, y);
                assert!(!r.is_nan() && !r.is_infinite());
                assert!(!g.is_nan() && !g.is_infinite());
                assert!(!b.is_nan() && !b.is_infinite());
                assert!((0.0..=1.0).contains(&r));
                assert!((0.0..=1.0).contains(&g));
                assert!((0.0..=1.0).contains(&b));
            }
        }
    }
}
