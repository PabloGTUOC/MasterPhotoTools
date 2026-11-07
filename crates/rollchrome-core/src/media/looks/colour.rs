//! Colour space conversions, CIELAB D50, and CIEDE2000 (DV-3).
//!
//! Clean-room implementations of:
//! - CIEDE2000 colour difference formula (Sharma, Wu, Dalal 2005).
//! - CIE 1976 L*a*b* under CIE Standard Illuminant D50.
//! - Bidirectional conversions between linear ProPhoto RGB and CIELAB D50.
//! - sRGB to CIELAB D50 via D65 -> D50 Bradford chromatic adaptation.
//! - JPEG ICC profile validation ensuring tagged colour space is sRGB.

use phototools_core::error::Error;

use crate::media::develop::colour;
use crate::media::develop::render::srgb_to_linear;

/// D50 reference white point in CIE 1931 XYZ (normalised Y = 1.0).
pub const D50_WHITE: [f32; 3] = [0.96422, 1.0, 0.82521];

/// Computes CIEDE2000 colour difference between two CIELAB coordinates.
///
/// Implemented according to Sharma, Wu, and Dalal (2005), "The CIEDE2000 Color-Difference
/// Formula: Implementation Notes, Supplementary Test Data, and Mathematical Observations".
pub fn ciede2000(lab1: [f32; 3], lab2: [f32; 3]) -> f32 {
    let [l1, a1, b1] = lab1;
    let [l2, a2, b2] = lab2;

    let c1_ab = (a1 * a1 + b1 * b1).sqrt();
    let c2_ab = (a2 * a2 + b2 * b2).sqrt();
    let c_bar_ab = 0.5 * (c1_ab + c2_ab);

    let c_bar_7 = c_bar_ab.powi(7);
    let g = 0.5 * (1.0 - (c_bar_7 / (c_bar_7 + 25.0_f32.powi(7))).sqrt());

    let a1_prime = (1.0 + g) * a1;
    let a2_prime = (1.0 + g) * a2;

    let c1_prime = (a1_prime * a1_prime + b1 * b1).sqrt();
    let c2_prime = (a2_prime * a2_prime + b2 * b2).sqrt();

    let hue_deg = |b: f32, a_prime: f32| -> f32 {
        if a_prime == 0.0 && b == 0.0 {
            0.0
        } else {
            let mut h = b.atan2(a_prime).to_degrees();
            if h < 0.0 {
                h += 360.0;
            }
            h
        }
    };

    let h1_prime = hue_deg(b1, a1_prime);
    let h2_prime = hue_deg(b2, a2_prime);

    let delta_l_prime = l2 - l1;
    let delta_c_prime = c2_prime - c1_prime;

    let delta_h_prime_deg = if c1_prime * c2_prime == 0.0 {
        0.0
    } else if (h2_prime - h1_prime).abs() <= 180.0 {
        h2_prime - h1_prime
    } else if h2_prime - h1_prime > 180.0 {
        (h2_prime - h1_prime) - 360.0
    } else {
        (h2_prime - h1_prime) + 360.0
    };

    let delta_big_h_prime =
        2.0 * (c1_prime * c2_prime).sqrt() * (delta_h_prime_deg.to_radians() * 0.5).sin();

    let l_bar_prime = 0.5 * (l1 + l2);
    let c_bar_prime = 0.5 * (c1_prime + c2_prime);

    let h_bar_prime = if c1_prime * c2_prime == 0.0 {
        h1_prime + h2_prime
    } else if (h1_prime - h2_prime).abs() <= 180.0 {
        0.5 * (h1_prime + h2_prime)
    } else if (h1_prime + h2_prime) < 360.0 {
        0.5 * (h1_prime + h2_prime + 360.0)
    } else {
        0.5 * (h1_prime + h2_prime - 360.0)
    };

    let t = 1.0 - 0.17 * (h_bar_prime - 30.0).to_radians().cos()
        + 0.24 * (2.0 * h_bar_prime).to_radians().cos()
        + 0.32 * (3.0 * h_bar_prime + 6.0).to_radians().cos()
        - 0.20 * (4.0 * h_bar_prime - 63.0).to_radians().cos();

    let delta_theta = 30.0 * (-(((h_bar_prime - 275.0) / 25.0).powi(2))).exp();
    let c_bar_prime_7 = c_bar_prime.powi(7);
    let r_c = 2.0 * (c_bar_prime_7 / (c_bar_prime_7 + 25.0_f32.powi(7))).sqrt();

    let l_minus_50_sq = (l_bar_prime - 50.0).powi(2);
    let s_l = 1.0 + (0.015 * l_minus_50_sq) / (20.0 + l_minus_50_sq).sqrt();
    let s_c = 1.0 + 0.045 * c_bar_prime;
    let s_h = 1.0 + 0.015 * c_bar_prime * t;

    let r_t = -(2.0 * delta_theta.to_radians()).sin() * r_c;

    let term_l = delta_l_prime / s_l;
    let term_c = delta_c_prime / s_c;
    let term_h = delta_big_h_prime / s_h;

    (term_l * term_l + term_c * term_c + term_h * term_h + r_t * term_c * term_h).sqrt()
}

#[inline]
fn lab_f(t: f32) -> f32 {
    let delta = 6.0 / 29.0;
    if t > delta * delta * delta {
        t.cbrt()
    } else {
        t / (3.0 * delta * delta) + 4.0 / 29.0
    }
}

#[inline]
fn inv_lab_f(t: f32) -> f32 {
    let delta = 6.0 / 29.0;
    if t > delta {
        t * t * t
    } else {
        3.0 * delta * delta * (t - 4.0 / 29.0)
    }
}

/// Converts CIE 1931 XYZ under D50 white point to CIELAB D50.
pub fn xyz_d50_to_lab(xyz: [f32; 3]) -> [f32; 3] {
    let fx = lab_f(xyz[0] / D50_WHITE[0]);
    let fy = lab_f(xyz[1] / D50_WHITE[1]);
    let fz = lab_f(xyz[2] / D50_WHITE[2]);

    let l = 116.0 * fy - 16.0;
    let a = 500.0 * (fx - fy);
    let b = 200.0 * (fy - fz);
    [l, a, b]
}

/// Converts CIELAB D50 to CIE 1931 XYZ under D50 white point.
pub fn lab_to_xyz_d50(lab: [f32; 3]) -> [f32; 3] {
    let [l, a, b] = lab;
    let fy = (l + 16.0) / 116.0;
    let fx = fy + a / 500.0;
    let fz = fy - b / 200.0;

    let x = D50_WHITE[0] * inv_lab_f(fx);
    let y = D50_WHITE[1] * inv_lab_f(fy);
    let z = D50_WHITE[2] * inv_lab_f(fz);
    [x, y, z]
}

/// Converts linear ProPhoto RGB to CIELAB D50.
pub fn prophoto_linear_to_lab_d50(pro: [f32; 3]) -> [f32; 3] {
    let m = colour::prophoto_to_xyz_d50();
    let xyz = colour::mul_3x3_vec(m, pro);
    xyz_d50_to_lab(xyz)
}

/// Inverted 3x3 matrix from XYZ D50 to linear ProPhoto RGB.
pub fn xyz_d50_to_prophoto_matrix() -> [[f32; 3]; 3] {
    let m = colour::prophoto_to_xyz_d50();
    colour::invert_3x3_f32(m).unwrap_or([[0.0; 3]; 3])
}

/// Converts CIELAB D50 back to linear ProPhoto RGB.
pub fn lab_d50_to_prophoto_linear(lab: [f32; 3]) -> [f32; 3] {
    let xyz = lab_to_xyz_d50(lab);
    let m_inv = xyz_d50_to_prophoto_matrix();
    colour::mul_3x3_vec(m_inv, xyz)
}

/// 3x3 Bradford chromatic adaptation matrix from D65 to D50.
pub fn bradford_d65_to_d50() -> [[f32; 3]; 3] {
    let b_50_to_65 = colour::bradford_d50_to_d65();
    let b_65_to_50_f64 = colour::invert_3x3_f64(b_50_to_65).unwrap_or([[0.0; 3]; 3]);
    [
        [
            b_65_to_50_f64[0][0] as f32,
            b_65_to_50_f64[0][1] as f32,
            b_65_to_50_f64[0][2] as f32,
        ],
        [
            b_65_to_50_f64[1][0] as f32,
            b_65_to_50_f64[1][1] as f32,
            b_65_to_50_f64[1][2] as f32,
        ],
        [
            b_65_to_50_f64[2][0] as f32,
            b_65_to_50_f64[2][1] as f32,
            b_65_to_50_f64[2][2] as f32,
        ],
    ]
}

/// Linear sRGB to XYZ D65 3x3 matrix.
pub fn srgb_linear_to_xyz_d65_matrix() -> [[f32; 3]; 3] {
    let m_f64 = colour::rgb_to_xyz_matrix(0.64, 0.33, 0.30, 0.60, 0.15, 0.06, 0.3127, 0.3290);
    [
        [m_f64[0][0] as f32, m_f64[0][1] as f32, m_f64[0][2] as f32],
        [m_f64[1][0] as f32, m_f64[1][1] as f32, m_f64[1][2] as f32],
        [m_f64[2][0] as f32, m_f64[2][1] as f32, m_f64[2][2] as f32],
    ]
}

/// Linear sRGB to XYZ D50 3x3 matrix (sRGB -> XYZ D65 -> Bradford -> XYZ D50).
pub fn srgb_linear_to_xyz_d50_matrix() -> [[f32; 3]; 3] {
    let srgb_to_d65 = srgb_linear_to_xyz_d65_matrix();
    let d65_to_d50 = bradford_d65_to_d50();
    colour::mul_3x3_f32(d65_to_d50, srgb_to_d65)
}

/// Converts sRGB in [0.0, 1.0] to CIELAB D50.
pub fn srgb_to_lab_d50(srgb: [f32; 3]) -> [f32; 3] {
    let lin_r = srgb_to_linear(srgb[0]);
    let lin_g = srgb_to_linear(srgb[1]);
    let lin_b = srgb_to_linear(srgb[2]);

    let m = srgb_linear_to_xyz_d50_matrix();
    let xyz = colour::mul_3x3_vec(m, [lin_r, lin_g, lin_b]);
    xyz_d50_to_lab(xyz)
}

/// Converts 8-bit sRGB in [0..=255] to CIELAB D50.
pub fn srgb_u8_to_lab_d50(srgb_u8: [u8; 3]) -> [f32; 3] {
    let srgb = [
        srgb_u8[0] as f32 / 255.0,
        srgb_u8[1] as f32 / 255.0,
        srgb_u8[2] as f32 / 255.0,
    ];
    srgb_to_lab_d50(srgb)
}

/// Verifies that if an embedded ICC profile exists in JPEG bytes, it represents sRGB.
pub fn verify_jpeg_srgb(jpeg_bytes: &[u8]) -> Result<(), Error> {
    // Look for APP2 ICC marker
    let marker = b"ICC_PROFILE\0";
    let mut chunks: Vec<(u8, u8, Vec<u8>)> = Vec::new();
    let mut i = 0;

    while i + 4 < jpeg_bytes.len() {
        if jpeg_bytes[i] == 0xFF && jpeg_bytes[i + 1] == 0xE2 {
            let length = ((jpeg_bytes[i + 2] as usize) << 8) | (jpeg_bytes[i + 3] as usize);
            if i + 2 + length <= jpeg_bytes.len() {
                let payload = &jpeg_bytes[i + 4..i + 2 + length];
                if payload.len() > 14 && &payload[..12] == marker {
                    let seq = payload[12];
                    let count = payload[13];
                    let data = payload[14..].to_vec();
                    chunks.push((seq, count, data));
                }
            }
            i += 2 + length;
        } else if jpeg_bytes[i] == 0xFF && jpeg_bytes[i + 1] == 0xDA {
            // SOS marker, start of scan
            break;
        } else {
            i += 1;
        }
    }

    if chunks.is_empty() {
        // No embedded ICC profile tag; default colour space is standard sRGB
        return Ok(());
    }

    chunks.sort_by_key(|c| c.0);
    let mut profile_bytes = Vec::new();
    for (_, _, data) in chunks {
        profile_bytes.extend_from_slice(&data);
    }

    let profile = moxcms::ColorProfile::new_from_slice(&profile_bytes)
        .map_err(|e| Error::Refused(format!("Failed to parse JPEG embedded ICC profile: {e:?}")))?;

    if let Some(desc) = profile.description {
        let is_srgb = match desc {
            moxcms::ProfileText::PlainString(s) => s.to_lowercase().contains("srgb"),
            moxcms::ProfileText::Localizable(locs) => {
                locs.iter().any(|l| l.value.to_lowercase().contains("srgb"))
            }
            moxcms::ProfileText::Description(d) => {
                d.ascii_string.to_lowercase().contains("srgb")
                    || d.unicode_string.to_lowercase().contains("srgb")
            }
        };
        if !is_srgb {
            return Err(Error::Refused(
                "JPEG embedded ICC profile is not sRGB".to_string(),
            ));
        }
    }

    Ok(())
}
