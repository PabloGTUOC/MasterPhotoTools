//! Color space conversions and perceptual operations (ED-9).
//!
//! Citations:
//! - Björn Ottosson (2020), "A perceptual color space for image processing" (Oklab / OkLCh).
//!   https://bottosson.github.io/posts/oklab/
//!
//! Oklab provides a perceptual color space designed specifically for image processing,
//! where Lightness (L), Chroma (C), and Hue angle (h) are separated predictably.
//! Rotating hue at constant L and C preserves perceptual lightness and saturation without
//! Rec.709 luma shifts or gamut skewing.

#![allow(clippy::excessive_precision)]

/// Convert linear sRGB [0.0, \infty) to Oklab (L, a, b).
///
/// Uses the M1 and M2 matrices from Björn Ottosson (2020).
#[inline]
pub fn linear_srgb_to_oklab(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let l = 0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b;
    let m = 0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b;
    let s = 0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b;

    let l_ = cbrt_signed(l);
    let m_ = cbrt_signed(m);
    let s_ = cbrt_signed(s);

    let oklab_l = 0.2104542553 * l_ + 0.7936177850 * m_ - 0.0040720468 * s_;
    let oklab_a = 1.9779984951 * l_ - 2.4285922050 * m_ + 0.4505937099 * s_;
    let oklab_b = 0.0259040371 * l_ + 0.7827717662 * m_ - 0.8086757660 * s_;

    (oklab_l, oklab_a, oklab_b)
}

/// Convert Oklab (L, a, b) to linear sRGB.
///
/// Uses the inverse matrices from Björn Ottosson (2020).
#[inline]
pub fn oklab_to_linear_srgb(l: f32, a: f32, b: f32) -> (f32, f32, f32) {
    let l_ = l + 0.3963377774 * a + 0.2158017575 * b;
    let m_ = l - 0.1055613458 * a - 0.0638541728 * b;
    let s_ = l - 0.0894841775 * a - 1.2914855480 * b;

    let l_cubed = l_ * l_ * l_;
    let m_cubed = m_ * m_ * m_;
    let s_cubed = s_ * s_ * s_;

    let r = 4.0767416621 * l_cubed - 3.3077115913 * m_cubed + 0.2309699292 * s_cubed;
    let g = -1.2684380046 * l_cubed + 2.6097574011 * m_cubed - 0.3413193965 * s_cubed;
    let b = -0.0041960863 * l_cubed - 0.7034186147 * m_cubed + 1.7076147010 * s_cubed;

    (r, g, b)
}

/// Pure Rust inlinable signed cube root using Halley's cubic convergence method.
/// Avoids non-inlined C runtime symbol calls (libSystem / libm) during pixel loops.
#[inline(always)]
fn cbrt_signed(x: f32) -> f32 {
    if x == 0.0 {
        return 0.0;
    }
    let is_neg = x < 0.0;
    let abs_x = if is_neg { -x } else { x };

    // Bit-hack initial guess for cube root (IEEE-754 exponent division by 3)
    let bits = abs_x.to_bits();
    let guess_bits = (bits / 3) + 0x2a514000;
    let mut y = f32::from_bits(guess_bits);

    // Halley's method: y_{n+1} = y * (y^3 + 2x) / (2y^3 + x)
    let y3 = y * y * y;
    y = y * (y3 + 2.0 * abs_x) / (2.0 * y3 + abs_x);
    let y3 = y * y * y;
    y = y * (y3 + 2.0 * abs_x) / (2.0 * y3 + abs_x);

    if is_neg {
        -y
    } else {
        y
    }
}

/// Rotates hue by delta_degrees in OkLCh at constant Lightness (L) and Chroma (C).
///
/// Preserves neutral grey identically. Uses 2D planar rotation matrix in (a, b) coordinates,
/// which preserves Euclidean chroma sqrt(a^2 + b^2) exactly while rotating hue angle.
#[inline]
pub fn rotate_hue_oklch(r: f32, g: f32, b: f32, delta_degrees: f32) -> (f32, f32, f32) {
    if delta_degrees == 0.0 {
        return (r, g, b);
    }
    let rad = delta_degrees.to_radians();
    let (sin, cos) = rad.sin_cos();
    rotate_hue_oklch_sincos(r, g, b, cos, sin)
}

/// Precomputes the 3x3 transfer matrix that combines M2, 2D planar hue rotation in (a, b),
/// and inverse M2 into a single linear map on (l_, m_, s_).
#[inline]
pub fn oklab_hue_rotation_matrix(cos: f32, sin: f32) -> [[f32; 3]; 3] {
    // M2 rows
    let m2_0 = [0.2104542553f32, 0.7936177850f32, -0.0040720468f32];
    let m2_1 = [1.9779984951f32, -2.4285922050f32, 0.4505937099f32];
    let m2_2 = [0.0259040371f32, 0.7827717662f32, -0.8086757660f32];

    // R * M2
    let mut rm = [[0.0f32; 3]; 3];
    rm[0] = m2_0;
    for col in 0..3 {
        rm[1][col] = m2_1[col] * cos - m2_2[col] * sin;
        rm[2][col] = m2_1[col] * sin + m2_2[col] * cos;
    }

    // M2_inv * (R * M2)
    let mut t = [[0.0f32; 3]; 3];
    for col in 0..3 {
        t[0][col] = rm[0][col] + 0.3963377774 * rm[1][col] + 0.2158017575 * rm[2][col];
        t[1][col] = rm[0][col] - 0.1055613458 * rm[1][col] - 0.0638541728 * rm[2][col];
        t[2][col] = rm[0][col] - 0.0894841775 * rm[1][col] - 1.2914855480 * rm[2][col];
    }
    t
}

/// Rotates hue using a precomputed 3x3 transfer matrix on (l_, m_, s_).
#[inline(always)]
pub fn rotate_hue_oklch_matrix(r: f32, g: f32, b: f32, mat: &[[f32; 3]; 3]) -> (f32, f32, f32) {
    let l = 0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b;
    let m = 0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b;
    let s = 0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b;

    let l_ = cbrt_signed(l);
    let m_ = cbrt_signed(m);
    let s_ = cbrt_signed(s);

    let l_out = mat[0][0] * l_ + mat[0][1] * m_ + mat[0][2] * s_;
    let m_out = mat[1][0] * l_ + mat[1][1] * m_ + mat[1][2] * s_;
    let s_out = mat[2][0] * l_ + mat[2][1] * m_ + mat[2][2] * s_;

    let l_cubed = l_out * l_out * l_out;
    let m_cubed = m_out * m_out * m_out;
    let s_cubed = s_out * s_out * s_out;

    let r_out = 4.0767416621 * l_cubed - 3.3077115913 * m_cubed + 0.2309699292 * s_cubed;
    let g_out = -1.2684380046 * l_cubed + 2.6097574011 * m_cubed - 0.3413193965 * s_cubed;
    let b_out = -0.0041960863 * l_cubed - 0.7034186147 * m_cubed + 1.7076147010 * s_cubed;

    (r_out, g_out, b_out)
}

/// Rotates hue in OkLCh with precomputed cos and sin of the delta angle.
#[inline]
pub fn rotate_hue_oklch_sincos(r: f32, g: f32, b: f32, cos: f32, sin: f32) -> (f32, f32, f32) {
    let (l, a, b_val) = linear_srgb_to_oklab(r, g, b);
    let new_a = a * cos - b_val * sin;
    let new_b = a * sin + b_val * cos;
    oklab_to_linear_srgb(l, new_a, new_b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oklab_round_trip_is_identity() {
        let colors = [
            (0.5, 0.5, 0.5),
            (1.0, 0.0, 0.0),
            (0.0, 1.0, 0.0),
            (0.0, 0.0, 1.0),
            (0.18, 0.18, 0.18),
            (0.8, 0.4, 0.2),
        ];
        for (r, g, b) in colors {
            let (l, a, b_val) = linear_srgb_to_oklab(r, g, b);
            let (r2, g2, b2) = oklab_to_linear_srgb(l, a, b_val);
            assert!(
                (r - r2).abs() < 1e-5 && (g - g2).abs() < 1e-5 && (b - b2).abs() < 1e-5,
                "Round trip failed for ({r}, {g}, {b}) -> ({r2}, {g2}, {b2})"
            );
        }
    }

    #[test]
    fn hue_rotation_matrix_matches_sincos() {
        let colors = [
            (0.5, 0.5, 0.5),
            (1.0, 0.0, 0.0),
            (0.0, 1.0, 0.0),
            (0.0, 0.0, 1.0),
            (0.18, 0.18, 0.18),
            (0.8, 0.4, 0.2),
        ];
        let angles = [10.0f32, -30.0, 45.0, 90.0, 180.0];
        for angle in angles {
            let rad = angle.to_radians();
            let (sin, cos) = rad.sin_cos();
            let mat = oklab_hue_rotation_matrix(cos, sin);
            for (r, g, b) in colors {
                let (r1, g1, b1) = rotate_hue_oklch_sincos(r, g, b, cos, sin);
                let (r2, g2, b2) = rotate_hue_oklch_matrix(r, g, b, &mat);
                assert!((r1 - r2).abs() < 1e-5, "r differs: {r1} vs {r2}");
                assert!((g1 - g2).abs() < 1e-5, "g differs: {g1} vs {g2}");
                assert!((b1 - b2).abs() < 1e-5, "b differs: {b1} vs {b2}");
            }
        }
    }

    #[test]
    fn hue_rotation_leaves_neutral_grey_untouched() {
        let (r, g, b) = (0.18, 0.18, 0.18);
        let (r2, g2, b2) = rotate_hue_oklch(r, g, b, 90.0);
        assert!((r - r2).abs() < 1e-6);
        assert!((g - g2).abs() < 1e-6);
        assert!((b - b2).abs() < 1e-6);
    }
}
