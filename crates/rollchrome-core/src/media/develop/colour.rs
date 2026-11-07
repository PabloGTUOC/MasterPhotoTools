//! Colour space transforms and camera calibration matrices (DV-1).
//!
//! Matrices are computed from published CIE 1931 xy chromaticity coordinates
//! in code, not hardcoded as opaque constants.
//!
//! Colour spaces:
//! - ProPhoto RGB (ROMM):
//!   R (0.7347, 0.2653), G (0.1596, 0.8404), B (0.0366, 0.0001), White D50 (0.3457, 0.3585)
//! - sRGB:
//!   R (0.64, 0.33), G (0.30, 0.60), B (0.15, 0.06), White D65 (0.3127, 0.3290)
//! - Bradford chromatic adaptation matrix:
//!   Converts between CIE D50 and D65 illuminants.

use phototools_core::error::Error;

/// Standard Bradford cone response matrix for chromatic adaptation.
const BRADFORD: [[f64; 3]; 3] = [
    [0.8951, 0.2664, -0.1614],
    [-0.7502, 1.7135, 0.0367],
    [0.0389, -0.0685, 1.0296],
];

/// ProPhoto primaries and D50 white point (ROMM RGB specification).
/// D50 white point normalized: X=0.96422, Y=1.00000, Z=0.82521.
const PROPHOTO_PRIMARIES: (f64, f64, f64, f64, f64, f64, f64, f64) = (
    0.7347,
    0.2653, // R
    0.1596,
    0.8404, // G
    0.0366,
    0.0001, // B
    0.96422 / (0.96422 + 1.0 + 0.82521),
    1.0 / (0.96422 + 1.0 + 0.82521), // White D50
);

/// sRGB primaries and D65 white point.
const SRGB_PRIMARIES: (f64, f64, f64, f64, f64, f64, f64, f64) = (
    0.64, 0.33, // R
    0.30, 0.60, // G
    0.15, 0.06, // B
    0.3127, 0.3290, // White D65
);

/// Computes a 3x3 RGB -> XYZ matrix from CIE xy chromaticity primaries and white point.
#[allow(clippy::too_many_arguments)]
pub fn rgb_to_xyz_matrix(
    rx: f64,
    ry: f64,
    gx: f64,
    gy: f64,
    bx: f64,
    by: f64,
    wx: f64,
    wy: f64,
) -> [[f64; 3]; 3] {
    let xr = rx / ry;
    let yr = 1.0;
    let zr = (1.0 - rx - ry) / ry;

    let xg = gx / gy;
    let yg = 1.0;
    let zg = (1.0 - gx - gy) / gy;

    let xb = bx / by;
    let yb = 1.0;
    let zb = (1.0 - bx - by) / by;

    let xw = wx / wy;
    let yw = 1.0;
    let zw = (1.0 - wx - wy) / wy;

    let m = [[xr, xg, xb], [yr, yg, yb], [zr, zg, zb]];
    let inv_m = invert_3x3_f64(m).unwrap_or([[0.0; 3]; 3]);
    let s = mul_mat_vec_3x3_f64(inv_m, [xw, yw, zw]);

    [
        [s[0] * xr, s[1] * xg, s[2] * xb],
        [s[0] * yr, s[1] * yg, s[2] * yb],
        [s[0] * zr, s[1] * zg, s[2] * zb],
    ]
}

/// Bradford chromatic adaptation transform matrix from D50 to D65 in XYZ space.
pub fn bradford_d50_to_d65() -> [[f64; 3]; 3] {
    let w_d50 = [0.3457 / 0.3585, 1.0, (1.0 - 0.3457 - 0.3585) / 0.3585];
    let w_d65 = [0.3127 / 0.3290, 1.0, (1.0 - 0.3127 - 0.3290) / 0.3290];

    let cone_d50 = mul_mat_vec_3x3_f64(BRADFORD, w_d50);
    let cone_d65 = mul_mat_vec_3x3_f64(BRADFORD, w_d65);

    let diag = [
        [cone_d65[0] / cone_d50[0], 0.0, 0.0],
        [0.0, cone_d65[1] / cone_d50[1], 0.0],
        [0.0, 0.0, cone_d65[2] / cone_d50[2]],
    ];

    let inv_b = invert_3x3_f64(BRADFORD).unwrap_or([[0.0; 3]; 3]);
    mul_3x3_f64(inv_b, mul_3x3_f64(diag, BRADFORD))
}

/// ProPhoto -> XYZ(D50) matrix computed from published primaries.
pub fn prophoto_to_xyz_d50() -> [[f32; 3]; 3] {
    let (rx, ry, gx, gy, bx, by, wx, wy) = PROPHOTO_PRIMARIES;
    let m = rgb_to_xyz_matrix(rx, ry, gx, gy, bx, by, wx, wy);
    to_f32_matrix(m)
}

/// ProPhoto -> XYZ(D65) matrix (ProPhoto -> XYZ(D50) followed by Bradford D50 -> D65).
pub fn prophoto_to_xyz_d65() -> [[f32; 3]; 3] {
    let (rx, ry, gx, gy, bx, by, wx, wy) = PROPHOTO_PRIMARIES;
    let m_d50 = rgb_to_xyz_matrix(rx, ry, gx, gy, bx, by, wx, wy);
    let cat = bradford_d50_to_d65();
    let m_d65 = mul_3x3_f64(cat, m_d50);
    to_f32_matrix(m_d65)
}

/// Linear ProPhoto -> Linear sRGB 3x3 matrix.
pub fn prophoto_to_srgb_matrix() -> [[f32; 3]; 3] {
    let (s_rx, s_ry, s_gx, s_gy, s_bx, s_by, s_wx, s_wy) = SRGB_PRIMARIES;
    let srgb_to_xyz = rgb_to_xyz_matrix(s_rx, s_ry, s_gx, s_gy, s_bx, s_by, s_wx, s_wy);
    let xyz_to_srgb = invert_3x3_f64(srgb_to_xyz).unwrap_or([[0.0; 3]; 3]);

    let (p_rx, p_ry, p_gx, p_gy, p_bx, p_by, p_wx, p_wy) = PROPHOTO_PRIMARIES;
    let prophoto_d50 = rgb_to_xyz_matrix(p_rx, p_ry, p_gx, p_gy, p_bx, p_by, p_wx, p_wy);
    let cat = bradford_d50_to_d65();
    let prophoto_d65 = mul_3x3_f64(cat, prophoto_d50);

    let m = mul_3x3_f64(xyz_to_srgb, prophoto_d65);
    to_f32_matrix(m)
}

/// Inverts `prophoto_to_srgb_matrix` to convert linear sRGB back to linear ProPhoto.
pub fn srgb_to_prophoto_matrix() -> [[f32; 3]; 3] {
    let m = prophoto_to_srgb_matrix();
    invert_3x3_f32(m).unwrap_or([[0.0; 3]; 3])
}

/// Constructs camera -> linear ProPhoto 3x3 matrix from DNG `xyz2cam` matrix.
///
/// Steps:
/// 1. `rgb2cam = xyz2cam · PROPHOTO_TO_XYZ_D65` (ProPhoto D50 adapted to D65).
/// 2. Normalise each row of `rgb2cam` to sum to 1, ensuring white-balanced camera
///    white maps to ProPhoto (1, 1, 1).
/// 3. Invert `rgb2cam`. Returns `Error::Refused` if the matrix is singular.
pub fn camera_to_prophoto(xyz2cam: [[f32; 3]; 3]) -> Result<[[f32; 3]; 3], Error> {
    let p_d65 = prophoto_to_xyz_d65();
    let mut rgb2cam = mul_3x3_f32(xyz2cam, p_d65);

    // Normalise each row to sum to 1
    for row in &mut rgb2cam {
        let row_sum = row[0] + row[1] + row[2];
        if row_sum.abs() < 1e-7 || !row_sum.is_finite() {
            return Err(Error::Refused(
                "Camera colour matrix row has zero sum or is not finite".to_string(),
            ));
        }
        row[0] /= row_sum;
        row[1] /= row_sum;
        row[2] /= row_sum;
    }

    match invert_3x3_f32(rgb2cam) {
        Some(cam2rgb) => Ok(cam2rgb),
        None => Err(Error::Refused(
            "Camera colour matrix is singular or non-invertible".to_string(),
        )),
    }
}

/// Multiplies a 3x3 matrix by a 3D vector.
#[inline]
pub fn mul_3x3_vec(m: [[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

fn to_f32_matrix(m: [[f64; 3]; 3]) -> [[f32; 3]; 3] {
    [
        [m[0][0] as f32, m[0][1] as f32, m[0][2] as f32],
        [m[1][0] as f32, m[1][1] as f32, m[1][2] as f32],
        [m[2][0] as f32, m[2][1] as f32, m[2][2] as f32],
    ]
}

fn mul_3x3_f64(a: [[f64; 3]; 3], b: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let mut out = [[0.0; 3]; 3];
    for r in 0..3 {
        for c in 0..3 {
            out[r][c] = a[r][0] * b[0][c] + a[r][1] * b[1][c] + a[r][2] * b[2][c];
        }
    }
    out
}

pub fn mul_3x3_f32(a: [[f32; 3]; 3], b: [[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let mut out = [[0.0; 3]; 3];
    for r in 0..3 {
        for c in 0..3 {
            out[r][c] = a[r][0] * b[0][c] + a[r][1] * b[1][c] + a[r][2] * b[2][c];
        }
    }
    out
}

fn mul_mat_vec_3x3_f64(m: [[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

pub fn invert_3x3_f64(m: [[f64; 3]; 3]) -> Option<[[f64; 3]; 3]> {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);

    if det.abs() < 1e-12 || !det.is_finite() {
        return None;
    }

    let inv_det = 1.0 / det;
    Some([
        [
            (m[1][1] * m[2][2] - m[1][2] * m[2][1]) * inv_det,
            (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * inv_det,
            (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * inv_det,
        ],
        [
            (m[1][2] * m[2][0] - m[1][0] * m[2][2]) * inv_det,
            (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * inv_det,
            (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * inv_det,
        ],
        [
            (m[1][0] * m[2][1] - m[1][1] * m[2][0]) * inv_det,
            (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * inv_det,
            (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * inv_det,
        ],
    ])
}

pub fn invert_3x3_f32(m: [[f32; 3]; 3]) -> Option<[[f32; 3]; 3]> {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);

    if det.abs() < 1e-7 || !det.is_finite() {
        return None;
    }

    let inv_det = 1.0 / det;
    Some([
        [
            (m[1][1] * m[2][2] - m[1][2] * m[2][1]) * inv_det,
            (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * inv_det,
            (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * inv_det,
        ],
        [
            (m[1][2] * m[2][0] - m[1][0] * m[2][2]) * inv_det,
            (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * inv_det,
            (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * inv_det,
        ],
        [
            (m[1][0] * m[2][1] - m[1][1] * m[2][0]) * inv_det,
            (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * inv_det,
            (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * inv_det,
        ],
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prophoto_matrix_matches_the_published_one() {
        let m = prophoto_to_xyz_d50();
        let expected = [
            [0.7976749, 0.1351917, 0.0313534],
            [0.2880402, 0.7118741, 0.0000857],
            [0.0, 0.0, 0.82521],
        ];
        for r in 0..3 {
            for c in 0..3 {
                let diff = (m[r][c] - expected[r][c]).abs();
                assert!(
                    diff <= 1e-4,
                    "m[{}][{}] = {}, expected {}, diff = {}",
                    r,
                    c,
                    m[r][c],
                    expected[r][c],
                    diff
                );
            }
        }
    }

    #[test]
    fn a_neutral_under_the_shot_white_balance_develops_to_equal_channels() {
        let test_cam_matrix = [[0.9, -0.3, -0.1], [-0.4, 1.2, 0.2], [-0.05, 0.15, 0.7]];
        let cam2prophoto = camera_to_prophoto(test_cam_matrix).expect("invertible camera matrix");

        let white_out = mul_3x3_vec(cam2prophoto, [1.0, 1.0, 1.0]);
        for (c, &val) in white_out.iter().enumerate() {
            assert!(
                (val - 1.0).abs() <= 1e-5,
                "white_out[{}] = {}, expected 1.0",
                c,
                val
            );
        }

        let grey_out = mul_3x3_vec(cam2prophoto, [0.18, 0.18, 0.18]);
        assert!((grey_out[0] - grey_out[1]).abs() <= 1e-5);
        assert!((grey_out[1] - grey_out[2]).abs() <= 1e-5);
        assert!((grey_out[0] - 0.18).abs() <= 1e-5);
    }

    #[test]
    fn prophoto_to_srgb_maps_white_grey_and_an_in_gamut_colour() {
        let prophoto_to_srgb = prophoto_to_srgb_matrix();
        let srgb_to_prophoto = srgb_to_prophoto_matrix();

        // 1. ProPhoto white [1, 1, 1] -> sRGB [255, 255, 255]
        let srgb_linear_white = mul_3x3_vec(prophoto_to_srgb, [1.0, 1.0, 1.0]);
        let srgb_u8_white = [
            linear_to_srgb8(srgb_linear_white[0]),
            linear_to_srgb8(srgb_linear_white[1]),
            linear_to_srgb8(srgb_linear_white[2]),
        ];
        assert_eq!(srgb_u8_white, [255, 255, 255]);

        // 2. Linear grey 0.18 -> sRGB 118 ± 1
        let grey_srgb = linear_to_srgb8(0.18);
        assert!(
            (grey_srgb as i32 - 118).abs() <= 1,
            "grey_srgb = {}, expected 118 ± 1",
            grey_srgb
        );

        // 3. ProPhoto colour made from sRGB (200, 80, 40) comes back as (200, 80, 40) ± 1
        let orig_srgb = [200u8, 80u8, 40u8];
        let orig_linear = [
            srgb8_to_linear(orig_srgb[0]),
            srgb8_to_linear(orig_srgb[1]),
            srgb8_to_linear(orig_srgb[2]),
        ];
        let prophoto_col = mul_3x3_vec(srgb_to_prophoto, orig_linear);
        let srgb_recovered_linear = mul_3x3_vec(prophoto_to_srgb, prophoto_col);
        let recovered_srgb = [
            linear_to_srgb8(srgb_recovered_linear[0]),
            linear_to_srgb8(srgb_recovered_linear[1]),
            linear_to_srgb8(srgb_recovered_linear[2]),
        ];
        for c in 0..3 {
            assert!(
                (recovered_srgb[c] as i32 - orig_srgb[c] as i32).abs() <= 1,
                "Channel {} mismatch: recovered {}, expected {}",
                c,
                recovered_srgb[c],
                orig_srgb[c]
            );
        }
    }

    fn linear_to_srgb8(v: f32) -> u8 {
        let v = v.clamp(0.0, 1.0);
        let s = if v <= 0.0031308 {
            12.92 * v
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        };
        (s * 255.0).round().clamp(0.0, 255.0) as u8
    }

    fn srgb8_to_linear(b: u8) -> f32 {
        let s = b as f32 / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    }
}
