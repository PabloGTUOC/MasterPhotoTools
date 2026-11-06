//! X-Trans sensor demosaicing implementation (DV-1).
//!
//! Demosaics Fujifilm 6x6 X-Trans colour filter arrays into 3-channel RGB.
//! Written strictly from the algorithm specification in DV-1 with no code
//! copied or translated from external projects (dcraw, LibRaw, darktable,
//! RawTherapee, Filmulator, or rawler's xtrans_fast.rs), which carry GPL/LGPL
//! licences incompatible with Rollchrome.
//!
//! Algorithm:
//! 1. Green everywhere: green photosites keep their recorded values. For red
//!    and blue photosites, missing green is estimated along four directions
//!    (horizontal, vertical, two diagonals) by linear interpolation between
//!    the nearest green samples within 4 pixels. Directions are ranked by a
//!    gradient (difference in green plus difference in same-colour samples,
//!    plus a curvature term |2*c - s1 - s2| when same-colour samples s1 and s2
//!    exist on both sides within 3 px, plus a green-curvature tie-break |near - far|
//!    between nearest and second-nearest greens within 4 px).
//!    Estimates with gradients within 10% (or 1e-6) of the minimum gradient
//!    are averaged.
//! 2. Red and blue: weighted by the edge's orientation measured on the finished
//!    green plane and edge-preserving green similarity. Green's gradient (gx, gy)
//!    is taken by central differences (one-sided at borders) and the structure
//!    tensor [gx^2, gx*gy; gx*gy, gy^2] is summed over the 5x5 window centred
//!    on the pixel. From its eigenvalues lambda1 >= lambda2, gradient direction
//!    theta and coherence C = ((lambda1 - lambda2) / (lambda1 + lambda2))^2 are derived.
//!    For each red or blue sample at offset (dx, dy) in the 5x5 window,
//!    across = dx*cos(theta) + dy*sin(theta), along = -dx*sin(theta) + dy*cos(theta),
//!    and weight w = (1 / (1 + along^2 + (1 + 24*C)*across^2)) * (1 / (1 + (dG / 0.02)^2))
//!    where dG = |G_sample - G_centre|.
//!    The weighted mean of colour differences (R - G or B - G) is added back to green.
//! 3. Edge safety: only valid samples within bounds are used near borders.

use rawler::imgop::Rect;
use rawler::pixarray::PixF32;
use rawler::CFA;
use rayon::prelude::*;

/// Demosaics raw X-Trans sensor float data within `roi` into RGB triples [R, G, B].
pub(crate) fn demosaic(pixels: &PixF32, cfa: &CFA, roi: Rect) -> Vec<[f32; 3]> {
    let w = roi.width();
    let h = roi.height();
    if w == 0 || h == 0 {
        return Vec::new();
    }

    // Shift CFA for ROI offset
    let cfa_roi = cfa.shift(roi.p.x % cfa.width, roi.p.y % cfa.height);

    let pix_w = pixels.width;
    let pix_h = pixels.height;
    let base_x = roi.p.x;
    let base_y = roi.p.y;

    // Helper to read sensor pixel within ROI
    let get_sensor = |r: usize, c: usize| -> f32 {
        let py = base_y + r;
        let px = base_x + c;
        if py < pix_h && px < pix_w {
            pixels.data[py * pix_w + px]
        } else {
            0.0
        }
    };

    // Step 1: Green everywhere
    // Pre-allocate green plane
    let green_plane: Vec<f32> = (0..h)
        .into_par_iter()
        .flat_map(|r| {
            let mut row_g = Vec::with_capacity(w);
            for c in 0..w {
                let col_id = cfa_roi.color_at(r, c);
                if col_id == 1 {
                    row_g.push(get_sensor(r, c));
                } else {
                    row_g.push(estimate_green(r, c, w, h, col_id, &cfa_roi, &get_sensor));
                }
            }
            row_g
        })
        .collect();

    // Step 2: Gradients of green plane by central differences (one-sided at borders)
    let mut gx = vec![0.0f32; w * h];
    let mut gy = vec![0.0f32; w * h];

    gx.par_chunks_exact_mut(w)
        .zip(gy.par_chunks_exact_mut(w))
        .enumerate()
        .for_each(|(r, (row_gx, row_gy))| {
            for c in 0..w {
                row_gx[c] = if w <= 1 {
                    0.0
                } else if c == 0 {
                    green_plane[r * w + 1] - green_plane[r * w]
                } else if c == w - 1 {
                    green_plane[r * w + w - 1] - green_plane[r * w + w - 2]
                } else {
                    (green_plane[r * w + c + 1] - green_plane[r * w + c - 1]) / 2.0
                };

                row_gy[c] = if h <= 1 {
                    0.0
                } else if r == 0 {
                    green_plane[w + c] - green_plane[c]
                } else if r == h - 1 {
                    green_plane[(h - 1) * w + c] - green_plane[(h - 2) * w + c]
                } else {
                    (green_plane[(r + 1) * w + c] - green_plane[(r - 1) * w + c]) / 2.0
                };
            }
        });

    // Step 3: Red and Blue weighted by orientation via structure tensor
    let rgb_pixels: Vec<[f32; 3]> = (0..h)
        .into_par_iter()
        .flat_map(|r| {
            let mut row_rgb = Vec::with_capacity(w);
            let row_offset = r * w;
            for c in 0..w {
                let col_id = cfa_roi.color_at(r, c);
                let g = green_plane[row_offset + c];

                let (r_val, b_val) = match col_id {
                    0 => {
                        let r_v = get_sensor(r, c);
                        let (cos_t, sin_t, coherence) =
                            compute_structure_tensor(r, c, w, h, &gx, &gy);
                        let b_v = estimate_chroma_channel(
                            r,
                            c,
                            w,
                            h,
                            2,
                            g,
                            cos_t,
                            sin_t,
                            coherence,
                            &green_plane,
                            &cfa_roi,
                            &get_sensor,
                        );
                        (r_v, b_v)
                    }
                    2 => {
                        let (cos_t, sin_t, coherence) =
                            compute_structure_tensor(r, c, w, h, &gx, &gy);
                        let r_v = estimate_chroma_channel(
                            r,
                            c,
                            w,
                            h,
                            0,
                            g,
                            cos_t,
                            sin_t,
                            coherence,
                            &green_plane,
                            &cfa_roi,
                            &get_sensor,
                        );
                        let b_v = get_sensor(r, c);
                        (r_v, b_v)
                    }
                    _ => {
                        let (cos_t, sin_t, coherence) =
                            compute_structure_tensor(r, c, w, h, &gx, &gy);
                        let r_v = estimate_chroma_channel(
                            r,
                            c,
                            w,
                            h,
                            0,
                            g,
                            cos_t,
                            sin_t,
                            coherence,
                            &green_plane,
                            &cfa_roi,
                            &get_sensor,
                        );
                        let b_v = estimate_chroma_channel(
                            r,
                            c,
                            w,
                            h,
                            2,
                            g,
                            cos_t,
                            sin_t,
                            coherence,
                            &green_plane,
                            &cfa_roi,
                            &get_sensor,
                        );
                        (r_v, b_v)
                    }
                };

                row_rgb.push([r_val, g, b_val]);
            }
            row_rgb
        })
        .collect();

    rgb_pixels
}

/// Direction vectors: (dr, dc)
const DIRECTIONS: [(isize, isize); 4] = [
    (0, 1),  // Horizontal
    (1, 0),  // Vertical
    (1, 1),  // Diagonal 1 (\)
    (1, -1), // Diagonal 2 (/)
];

#[inline]
#[allow(clippy::too_many_arguments)]
fn estimate_green<F>(
    r: usize,
    c: usize,
    w: usize,
    h: usize,
    centre_color: usize,
    cfa_roi: &CFA,
    get_sensor: &F,
) -> f32
where
    F: Fn(usize, usize) -> f32 + Sync,
{
    let mut estimates = Vec::with_capacity(4);
    let mut gradients = Vec::with_capacity(4);

    let ir = r as isize;
    let ic = c as isize;
    let ih = h as isize;
    let iw = w as isize;

    for &(dr, dc) in &DIRECTIONS {
        // Nearest and second-nearest green on positive side within 4 pixels
        let mut pos_greens = Vec::with_capacity(2);
        for k in 1..=4isize {
            let nr = ir + k * dr;
            let nc = ic + k * dc;
            if nr >= 0 && nr < ih && nc >= 0 && nc < iw {
                let ur = nr as usize;
                let uc = nc as usize;
                if cfa_roi.color_at(ur, uc) == 1 {
                    pos_greens.push((k, get_sensor(ur, uc)));
                    if pos_greens.len() == 2 {
                        break;
                    }
                }
            }
        }

        // Nearest and second-nearest green on negative side within 4 pixels
        let mut neg_greens = Vec::with_capacity(2);
        for k in 1..=4isize {
            let nr = ir - k * dr;
            let nc = ic - k * dc;
            if nr >= 0 && nr < ih && nc >= 0 && nc < iw {
                let ur = nr as usize;
                let uc = nc as usize;
                if cfa_roi.color_at(ur, uc) == 1 {
                    neg_greens.push((k, get_sensor(ur, uc)));
                    if neg_greens.len() == 2 {
                        break;
                    }
                }
            }
        }

        if !pos_greens.is_empty() && !neg_greens.is_empty() {
            let (kp, gp) = pos_greens[0];
            let (kn, gn) = neg_greens[0];
            let near = (kn as f32 * gp + kp as f32 * gn) / (kp + kn) as f32;
            let mut est = near;

            let curv_g = if pos_greens.len() >= 2 && neg_greens.len() >= 2 {
                let (k2_p, g2_p) = pos_greens[1];
                let (k2_n, g2_n) = neg_greens[1];
                let far = (k2_n as f32 * g2_p + k2_p as f32 * g2_n) / (k2_p + k2_n) as f32;
                (near - far).abs()
            } else {
                0.0
            };

            // Nearest same-colour sample on positive side within 3 pixels
            let mut pos_same = None;
            for m in 1..=3isize {
                let nr = ir + m * dr;
                let nc = ic + m * dc;
                if nr >= 0 && nr < ih && nc >= 0 && nc < iw {
                    let ur = nr as usize;
                    let uc = nc as usize;
                    if cfa_roi.color_at(ur, uc) == centre_color {
                        pos_same = Some((m, get_sensor(ur, uc)));
                        break;
                    }
                }
            }

            // Nearest same-colour sample on negative side within 3 pixels
            let mut neg_same = None;
            for m in 1..=3isize {
                let nr = ir - m * dr;
                let nc = ic - m * dc;
                if nr >= 0 && nr < ih && nc >= 0 && nc < iw {
                    let ur = nr as usize;
                    let uc = nc as usize;
                    if cfa_roi.color_at(ur, uc) == centre_color {
                        neg_same = Some((m, get_sensor(ur, uc)));
                        break;
                    }
                }
            }

            let centre_val = get_sensor(r, c);
            let same_diff = match (pos_same, neg_same) {
                (Some((_, sp)), Some((_, sn))) => (sp - sn).abs(),
                (Some((_, sp)), None) => (sp - centre_val).abs(),
                (None, Some((_, sn))) => (sn - centre_val).abs(),
                (None, None) => 0.0,
            };

            let curvature = match (pos_same, neg_same) {
                (Some((_, sp)), Some((_, sn))) => (2.0 * centre_val - sp - sn).abs(),
                _ => 0.0,
            };

            // Estimate correction: only when s1 and s2 sit at the same distance as g1 and g2
            if let (Some((mp, sp)), Some((mn, sn))) = (pos_same, neg_same) {
                if mp == mn && kp == kn {
                    est = (gp + gn) / 2.0 + (2.0 * centre_val - sp - sn) / 4.0;
                }
            }

            let grad = (gp - gn).abs() + same_diff + curvature + curv_g;
            estimates.push(est);
            gradients.push(grad);
        }
    }

    if estimates.is_empty() {
        // Fallback: average any green samples in the 5x5 window
        let mut sum_g = 0.0f32;
        let mut count_g = 0;
        for dr in -2..=2isize {
            for dc in -2..=2isize {
                let nr = ir + dr;
                let nc = ic + dc;
                if nr >= 0 && nr < ih && nc >= 0 && nc < iw {
                    let ur = nr as usize;
                    let uc = nc as usize;
                    if cfa_roi.color_at(ur, uc) == 1 {
                        sum_g += get_sensor(ur, uc);
                        count_g += 1;
                    }
                }
            }
        }
        if count_g > 0 {
            sum_g / count_g as f32
        } else {
            get_sensor(r, c)
        }
    } else {
        let mut min_grad = gradients[0];
        for &g in &gradients[1..] {
            if g < min_grad {
                min_grad = g;
            }
        }

        let threshold = min_grad + (0.10 * min_grad).max(1e-6);
        let mut sum_est = 0.0f32;
        let mut count = 0;
        for i in 0..estimates.len() {
            if gradients[i] <= threshold {
                sum_est += estimates[i];
                count += 1;
            }
        }
        if count > 0 {
            sum_est / count as f32
        } else {
            estimates[0]
        }
    }
}

#[inline]
fn compute_structure_tensor(
    r: usize,
    c: usize,
    w: usize,
    h: usize,
    gx: &[f32],
    gy: &[f32],
) -> (f32, f32, f32) {
    let ir = r as isize;
    let ic = c as isize;
    let ih = h as isize;
    let iw = w as isize;

    let mut jxx = 0.0f32;
    let mut jxy = 0.0f32;
    let mut jyy = 0.0f32;
    for dr in -2..=2isize {
        for dc in -2..=2isize {
            let nr = ir + dr;
            let nc = ic + dc;
            if nr >= 0 && nr < ih && nc >= 0 && nc < iw {
                let ur = nr as usize;
                let uc = nc as usize;
                let idx = ur * w + uc;
                let vx = gx[idx];
                let vy = gy[idx];
                jxx += vx * vx;
                jxy += vx * vy;
                jyy += vy * vy;
            }
        }
    }

    let tr = jxx + jyy;
    let disc = ((jxx - jyy).powi(2) + 4.0 * jxy * jxy).max(0.0);
    let sqrt_disc = disc.sqrt();
    let l1 = (tr + sqrt_disc) / 2.0;
    let l2 = (tr - sqrt_disc) / 2.0;

    let coherence = if (l1 + l2) < 1e-12 {
        0.0
    } else {
        ((l1 - l2) / (l1 + l2)).powi(2)
    };

    let theta = 0.5 * (2.0 * jxy).atan2(jxx - jyy);
    (theta.cos(), theta.sin(), coherence)
}

#[inline]
#[allow(clippy::too_many_arguments)]
fn estimate_chroma_channel<F>(
    r: usize,
    c: usize,
    w: usize,
    h: usize,
    target_channel: usize,
    g_centre: f32,
    cos_t: f32,
    sin_t: f32,
    coherence: f32,
    green_plane: &[f32],
    cfa_roi: &CFA,
    get_sensor: &F,
) -> f32
where
    F: Fn(usize, usize) -> f32 + Sync,
{
    let ir = r as isize;
    let ic = c as isize;
    let ih = h as isize;
    let iw = w as isize;

    let mut weight_sum = 0.0f32;
    let mut weighted_diff_sum = 0.0f32;

    for dr in -2..=2isize {
        for dc in -2..=2isize {
            let nr = ir + dr;
            let nc = ic + dc;
            if nr >= 0 && nr < ih && nc >= 0 && nc < iw {
                let ur = nr as usize;
                let uc = nc as usize;
                if cfa_roi.color_at(ur, uc) == target_channel {
                    let dx = dc as f32;
                    let dy = dr as f32;
                    let across = dx * cos_t + dy * sin_t;
                    let along = -dx * sin_t + dy * cos_t;
                    let w_orientation =
                        1.0 / (1.0 + along * along + (1.0 + 24.0 * coherence) * across * across);
                    let dg = (green_plane[ur * w + uc] - g_centre).abs();
                    let dg_term = dg / 0.02;
                    let w_edge = 1.0 / (1.0 + dg_term * dg_term);
                    let w_val = w_orientation * w_edge;
                    let diff = get_sensor(ur, uc) - green_plane[ur * w + uc];
                    weight_sum += w_val;
                    weighted_diff_sum += w_val * diff;
                }
            }
        }
    }

    if weight_sum > 0.0 {
        g_centre + weighted_diff_sum / weight_sum
    } else {
        g_centre
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn xtrans_cfa() -> CFA {
        CFA::new("GGRGGBGGBGGRBRGRBGGGBGGRGGRGGBRBGBRG")
    }

    #[test]
    fn a_flat_colour_field_comes_back_unchanged_through_the_xtrans_demosaic() {
        let w = 48;
        let h = 48;
        let cfa = xtrans_cfa();
        let flat_rgb = [0.2f32, 0.5, 0.3];

        let mut sensor_data = Vec::with_capacity(w * h);
        for r in 0..h {
            for c in 0..w {
                let ch = cfa.color_at(r, c);
                sensor_data.push(flat_rgb[ch]);
            }
        }

        let pix = PixF32::new_with(sensor_data, w, h);
        let roi = Rect::new(
            rawler::imgop::Point::new(0, 0),
            rawler::imgop::Dim2::new(w, h),
        );
        let result = demosaic(&pix, &cfa, roi);

        assert_eq!(result.len(), w * h);
        for (i, p) in result.iter().enumerate() {
            let r = i / w;
            let c = i % w;
            for ch in 0..3 {
                let diff = (p[ch] - flat_rgb[ch]).abs();
                assert!(
                    diff <= 1e-5,
                    "Pixel ({}, {}) ch {}: got {}, expected {}, diff {}",
                    r,
                    c,
                    ch,
                    p[ch],
                    flat_rgb[ch],
                    diff
                );
            }
        }
    }

    #[test]
    fn the_demosaic_is_the_same_at_every_offset_of_the_pattern() {
        let w = 48;
        let h = 48;
        let cfa = xtrans_cfa();
        let flat_rgb = [0.2f32, 0.5, 0.3];

        for shift_y in 0..6 {
            for shift_x in 0..6 {
                let shifted_cfa = cfa.shift(shift_x, shift_y);
                let mut sensor_data = Vec::with_capacity(w * h);
                for r in 0..h {
                    for c in 0..w {
                        let ch = shifted_cfa.color_at(r, c);
                        sensor_data.push(flat_rgb[ch]);
                    }
                }

                let pix = PixF32::new_with(sensor_data, w, h);
                let roi = Rect::new(
                    rawler::imgop::Point::new(0, 0),
                    rawler::imgop::Dim2::new(w, h),
                );
                let result = demosaic(&pix, &shifted_cfa, roi);

                for (i, p) in result.iter().enumerate() {
                    let r = i / w;
                    let c = i % w;
                    for ch in 0..3 {
                        let diff = (p[ch] - flat_rgb[ch]).abs();
                        assert!(
                            diff <= 1e-5,
                            "Shift ({}, {}) pixel ({}, {}) ch {}: got {}, expected {}",
                            shift_x,
                            shift_y,
                            r,
                            c,
                            ch,
                            p[ch],
                            flat_rgb[ch]
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_smooth_ramp_comes_back_within_half_a_percent() {
        let size = 96;
        let cfa = xtrans_cfa();

        // 1. Horizontal ramp 0.05 -> 0.95
        let mut h_sensor = Vec::with_capacity(size * size);
        for _r in 0..size {
            for c in 0..size {
                let val = 0.05 + 0.90 * (c as f32 / (size - 1) as f32);
                h_sensor.push(val);
            }
        }
        let pix_h = PixF32::new_with(h_sensor, size, size);
        let roi = Rect::new(
            rawler::imgop::Point::new(0, 0),
            rawler::imgop::Dim2::new(size, size),
        );
        let res_h = demosaic(&pix_h, &cfa, roi);

        for r in 3..(size - 3) {
            for c in 3..(size - 3) {
                let expected = 0.05 + 0.90 * (c as f32 / (size - 1) as f32);
                let p = res_h[r * size + c];
                for (ch, &val) in p.iter().enumerate() {
                    let diff = (val - expected).abs();
                    assert!(
                        diff <= 0.005,
                        "H-ramp at ({},{}) ch {}: got {}, expected {}, diff {}",
                        r,
                        c,
                        ch,
                        val,
                        expected,
                        diff
                    );
                }
            }
        }

        // 2. Vertical ramp 0.05 -> 0.95
        let mut v_sensor = Vec::with_capacity(size * size);
        for r in 0..size {
            for _c in 0..size {
                let val = 0.05 + 0.90 * (r as f32 / (size - 1) as f32);
                v_sensor.push(val);
            }
        }
        let pix_v = PixF32::new_with(v_sensor, size, size);
        let res_v = demosaic(&pix_v, &cfa, roi);

        for r in 3..(size - 3) {
            for c in 3..(size - 3) {
                let expected = 0.05 + 0.90 * (r as f32 / (size - 1) as f32);
                let p = res_v[r * size + c];
                for (ch, &val) in p.iter().enumerate() {
                    let diff = (val - expected).abs();
                    assert!(
                        diff <= 0.005,
                        "V-ramp at ({},{}) ch {}: got {}, expected {}, diff {}",
                        r,
                        c,
                        ch,
                        val,
                        expected,
                        diff
                    );
                }
            }
        }
    }

    #[test]
    fn a_fine_grating_comes_back_without_a_maze() {
        let size = 96;
        let cfa = xtrans_cfa();
        let period = 8.0f32;
        let roi = Rect::new(
            rawler::imgop::Point::new(0, 0),
            rawler::imgop::Dim2::new(size, size),
        );

        // Test at 0° (horizontal grating, varies with x)
        let mut h_sensor = Vec::with_capacity(size * size);
        for _r in 0..size {
            for c in 0..size {
                let val = 0.3 + 0.2 * (2.0 * std::f32::consts::PI * c as f32 / period).sin();
                h_sensor.push(val);
            }
        }
        let pix_h = PixF32::new_with(h_sensor, size, size);
        let res_h = demosaic(&pix_h, &cfa, roi);

        let mut sum_sq_h = [0.0f32; 3];
        let mut count_h = 0;
        let mut h_max = [0.0f32; 3];
        for r in 4..(size - 4) {
            for c in 4..(size - 4) {
                let expected = 0.3 + 0.2 * (2.0 * std::f32::consts::PI * c as f32 / period).sin();
                let p = res_h[r * size + c];
                for ch in 0..3 {
                    let d = (p[ch] - expected).abs();
                    sum_sq_h[ch] += d * d;
                    if d > h_max[ch] {
                        h_max[ch] = d;
                    }
                }
                count_h += 1;
            }
        }
        let rms_h = [
            (sum_sq_h[0] / count_h as f32).sqrt(),
            (sum_sq_h[1] / count_h as f32).sqrt(),
            (sum_sq_h[2] / count_h as f32).sqrt(),
        ];
        println!(
            "Test 4 0° grating: RMS=[R:{:.6}, G:{:.6}, B:{:.6}], MAX=[R:{:.6}, G:{:.6}, B:{:.6}]",
            rms_h[0], rms_h[1], rms_h[2], h_max[0], h_max[1], h_max[2]
        );
        for (ch, &val) in rms_h.iter().enumerate() {
            assert!(val <= 0.006, "0° grating RMS ch {} = {} > 0.006", ch, val);
        }

        // Test at 90° (vertical grating, varies with y)
        let mut v_sensor = Vec::with_capacity(size * size);
        for r in 0..size {
            for _c in 0..size {
                let val = 0.3 + 0.2 * (2.0 * std::f32::consts::PI * r as f32 / period).sin();
                v_sensor.push(val);
            }
        }
        let pix_v = PixF32::new_with(v_sensor, size, size);
        let res_v = demosaic(&pix_v, &cfa, roi);

        let mut sum_sq_v = [0.0f32; 3];
        let mut count_v = 0;
        let mut v_max = [0.0f32; 3];
        for r in 4..(size - 4) {
            for c in 4..(size - 4) {
                let expected = 0.3 + 0.2 * (2.0 * std::f32::consts::PI * r as f32 / period).sin();
                let p = res_v[r * size + c];
                for ch in 0..3 {
                    let diff = (p[ch] - expected).abs();
                    sum_sq_v[ch] += diff * diff;
                    if diff > v_max[ch] {
                        v_max[ch] = diff;
                    }
                }
                count_v += 1;
            }
        }
        let rms_v = [
            (sum_sq_v[0] / count_v as f32).sqrt(),
            (sum_sq_v[1] / count_v as f32).sqrt(),
            (sum_sq_v[2] / count_v as f32).sqrt(),
        ];
        println!(
            "Test 4 90° grating: RMS=[R:{:.6}, G:{:.6}, B:{:.6}], MAX=[R:{:.6}, G:{:.6}, B:{:.6}]",
            rms_v[0], rms_v[1], rms_v[2], v_max[0], v_max[1], v_max[2]
        );
        for (ch, &val) in rms_v.iter().enumerate() {
            assert!(val <= 0.006, "90° grating RMS ch {} = {} > 0.006", ch, val);
        }
    }

    #[test]
    fn a_fine_colour_grating_comes_back_without_false_colour() {
        use std::f32::consts::PI;
        let size = 96;
        let cfa = xtrans_cfa();
        // Period is 16 px (moved from 8 px in Review round 3: period 8 is near the sampling limit of sparse red/blue photosites, while natural colour detail is coarser).
        let period = 16.0f32;
        let roi = Rect::new(
            rawler::imgop::Point::new(0, 0),
            rawler::imgop::Dim2::new(size, size),
        );
        let phases = [0.0f32, 2.0 * PI / 3.0, 4.0 * PI / 3.0];

        // 0° grating (horizontal, varies with c)
        let mut h_sensor = Vec::with_capacity(size * size);
        for _r in 0..size {
            for c in 0..size {
                let ch = cfa.color_at(_r, c);
                let val = 0.3 + 0.2 * (2.0 * PI * c as f32 / period + phases[ch]).sin();
                h_sensor.push(val);
            }
        }
        let res_h = demosaic(&PixF32::new_with(h_sensor, size, size), &cfa, roi);
        let mut h_sum_sq = [0.0f32; 3];
        let mut count = 0;
        let mut h_max = [0.0f32; 3];
        for r in 4..(size - 4) {
            for c in 4..(size - 4) {
                let p = res_h[r * size + c];
                for ch in 0..3 {
                    let expected = 0.3 + 0.2 * (2.0 * PI * c as f32 / period + phases[ch]).sin();
                    let diff = (p[ch] - expected).abs();
                    h_sum_sq[ch] += diff * diff;
                    if diff > h_max[ch] {
                        h_max[ch] = diff;
                    }
                }
                count += 1;
            }
        }
        let h_rms = [
            (h_sum_sq[0] / count as f32).sqrt(),
            (h_sum_sq[1] / count as f32).sqrt(),
            (h_sum_sq[2] / count as f32).sqrt(),
        ];
        println!(
            "Test 5 0°: RMS=[R:{:.6}, G:{:.6}, B:{:.6}], MAX=[R:{:.6}, G:{:.6}, B:{:.6}]",
            h_rms[0], h_rms[1], h_rms[2], h_max[0], h_max[1], h_max[2]
        );

        // 90° grating (vertical, varies with r)
        let mut v_sensor = Vec::with_capacity(size * size);
        for r in 0..size {
            for _c in 0..size {
                let ch = cfa.color_at(r, _c);
                let val = 0.3 + 0.2 * (2.0 * PI * r as f32 / period + phases[ch]).sin();
                v_sensor.push(val);
            }
        }
        let res_v = demosaic(&PixF32::new_with(v_sensor, size, size), &cfa, roi);
        let mut v_sum_sq = [0.0f32; 3];
        let mut v_max = [0.0f32; 3];
        for r in 4..(size - 4) {
            for c in 4..(size - 4) {
                let p = res_v[r * size + c];
                for ch in 0..3 {
                    let expected = 0.3 + 0.2 * (2.0 * PI * r as f32 / period + phases[ch]).sin();
                    let diff = (p[ch] - expected).abs();
                    v_sum_sq[ch] += diff * diff;
                    if diff > v_max[ch] {
                        v_max[ch] = diff;
                    }
                }
            }
        }
        let v_rms = [
            (v_sum_sq[0] / count as f32).sqrt(),
            (v_sum_sq[1] / count as f32).sqrt(),
            (v_sum_sq[2] / count as f32).sqrt(),
        ];
        println!(
            "Test 5 90°: RMS=[R:{:.6}, G:{:.6}, B:{:.6}], MAX=[R:{:.6}, G:{:.6}, B:{:.6}]",
            v_rms[0], v_rms[1], v_rms[2], v_max[0], v_max[1], v_max[2]
        );

        // 45° grating (diagonal, varies with (r + c)/sqrt(2))
        let mut d_sensor = Vec::with_capacity(size * size);
        for r in 0..size {
            for c in 0..size {
                let ch = cfa.color_at(r, c);
                let u = (r + c) as f32 / std::f32::consts::SQRT_2;
                let val = 0.3 + 0.2 * (2.0 * PI * u / period + phases[ch]).sin();
                d_sensor.push(val);
            }
        }
        let res_d = demosaic(&PixF32::new_with(d_sensor, size, size), &cfa, roi);
        let mut d_sum_sq = [0.0f32; 3];
        let mut d_max = [0.0f32; 3];
        for r in 4..(size - 4) {
            for c in 4..(size - 4) {
                let u = (r + c) as f32 / std::f32::consts::SQRT_2;
                let p = res_d[r * size + c];
                for ch in 0..3 {
                    let expected = 0.3 + 0.2 * (2.0 * PI * u / period + phases[ch]).sin();
                    let diff = (p[ch] - expected).abs();
                    d_sum_sq[ch] += diff * diff;
                    if diff > d_max[ch] {
                        d_max[ch] = diff;
                    }
                }
            }
        }
        let d_rms = [
            (d_sum_sq[0] / count as f32).sqrt(),
            (d_sum_sq[1] / count as f32).sqrt(),
            (d_sum_sq[2] / count as f32).sqrt(),
        ];
        println!(
            "Test 5 45°: RMS=[R:{:.6}, G:{:.6}, B:{:.6}], MAX=[R:{:.6}, G:{:.6}, B:{:.6}]",
            d_rms[0], d_rms[1], d_rms[2], d_max[0], d_max[1], d_max[2]
        );

        // Assertions for Test 5
        assert!(
            h_rms[1] <= 0.006,
            "0° colour grating G RMS = {} > 0.006",
            h_rms[1]
        );
        assert!(
            h_rms[0] <= 0.020,
            "0° colour grating R RMS = {} > 0.020",
            h_rms[0]
        );
        assert!(
            h_rms[2] <= 0.020,
            "0° colour grating B RMS = {} > 0.020",
            h_rms[2]
        );
        assert!(
            v_rms[1] <= 0.006,
            "90° colour grating G RMS = {} > 0.006",
            v_rms[1]
        );
        assert!(
            v_rms[0] <= 0.020,
            "90° colour grating R RMS = {} > 0.020",
            v_rms[0]
        );
        assert!(
            v_rms[2] <= 0.020,
            "90° colour grating B RMS = {} > 0.020",
            v_rms[2]
        );
        assert!(
            d_rms[1] <= 0.006,
            "45° colour grating G RMS = {} > 0.006",
            d_rms[1]
        );
        assert!(
            d_rms[0] <= 0.020,
            "45° colour grating R RMS = {} > 0.020",
            d_rms[0]
        );
        assert!(
            d_rms[2] <= 0.020,
            "45° colour grating B RMS = {} > 0.020",
            d_rms[2]
        );
        assert!(
            d_rms[1] <= 0.006,
            "45° colour grating G RMS = {} > 0.006",
            d_rms[1]
        );
        assert!(
            d_rms[0] <= 0.020,
            "45° colour grating R RMS = {} > 0.020",
            d_rms[0]
        );
        assert!(
            d_rms[2] <= 0.020,
            "45° colour grating B RMS = {} > 0.020",
            d_rms[2]
        );
    }

    #[test]
    fn a_colour_step_edge_stays_sharp_and_unfringed() {
        let size = 96;
        let cfa = xtrans_cfa();
        let col1 = [0.2f32, 0.5, 0.3];
        let col2 = [0.6f32, 0.4, 0.1];
        let roi = Rect::new(
            rawler::imgop::Point::new(0, 0),
            rawler::imgop::Dim2::new(size, size),
        );

        // 1. Vertical step edge at c = 48
        let mut v_sensor = Vec::with_capacity(size * size);
        for r in 0..size {
            for c in 0..size {
                let ch = cfa.color_at(r, c);
                let col = if c < 48 { col1 } else { col2 };
                v_sensor.push(col[ch]);
            }
        }
        let res_v = demosaic(&PixF32::new_with(v_sensor, size, size), &cfa, roi);
        let mut v_far_max = [0.0f32; 3];
        let mut v_far_sq = [0.0f32; 3];
        let mut v_far_cnt = 0;
        let mut v_near_max = [0.0f32; 3];
        let mut v_near_sq = [0.0f32; 3];
        let mut v_near_cnt = 0;
        for r in 4..(size - 4) {
            for c in 4..(size - 4) {
                let expected = if c < 48 { col1 } else { col2 };
                let p = res_v[r * size + c];
                let dist = (c as isize - 48).abs();
                if dist > 3 {
                    for ch in 0..3 {
                        let diff = (p[ch] - expected[ch]).abs();
                        v_far_sq[ch] += diff * diff;
                        if diff > v_far_max[ch] {
                            v_far_max[ch] = diff;
                        }
                    }
                    v_far_cnt += 1;
                } else {
                    for ch in 0..3 {
                        let diff = (p[ch] - expected[ch]).abs();
                        v_near_sq[ch] += diff * diff;
                        if diff > v_near_max[ch] {
                            v_near_max[ch] = diff;
                        }
                    }
                    v_near_cnt += 1;
                }
            }
        }
        let v_far_rms = [
            (v_far_sq[0] / v_far_cnt as f32).sqrt(),
            (v_far_sq[1] / v_far_cnt as f32).sqrt(),
            (v_far_sq[2] / v_far_cnt as f32).sqrt(),
        ];
        let v_near_rms = [
            (v_near_sq[0] / v_near_cnt as f32).sqrt(),
            (v_near_sq[1] / v_near_cnt as f32).sqrt(),
            (v_near_sq[2] / v_near_cnt as f32).sqrt(),
        ];
        println!("Test 6 Vertical far-field (>3px): RMS=[R:{:.6}, G:{:.6}, B:{:.6}], MAX=[R:{:.6}, G:{:.6}, B:{:.6}]",
            v_far_rms[0], v_far_rms[1], v_far_rms[2], v_far_max[0], v_far_max[1], v_far_max[2]);
        println!("Test 6 Vertical near-edge (<=3px): RMS=[R:{:.6}, G:{:.6}, B:{:.6}], MAX=[R:{:.6}, G:{:.6}, B:{:.6}]",
            v_near_rms[0], v_near_rms[1], v_near_rms[2], v_near_max[0], v_near_max[1], v_near_max[2]);

        // 2. Horizontal step edge at r = 48
        let mut h_sensor = Vec::with_capacity(size * size);
        for r in 0..size {
            for c in 0..size {
                let ch = cfa.color_at(r, c);
                let col = if r < 48 { col1 } else { col2 };
                h_sensor.push(col[ch]);
            }
        }
        let res_h = demosaic(&PixF32::new_with(h_sensor, size, size), &cfa, roi);
        let mut h_far_max = [0.0f32; 3];
        let mut h_far_sq = [0.0f32; 3];
        let mut h_far_cnt = 0;
        let mut h_near_max = [0.0f32; 3];
        let mut h_near_sq = [0.0f32; 3];
        let mut h_near_cnt = 0;
        for r in 4..(size - 4) {
            for c in 4..(size - 4) {
                let expected = if r < 48 { col1 } else { col2 };
                let p = res_h[r * size + c];
                let dist = (r as isize - 48).abs();
                if dist > 3 {
                    for ch in 0..3 {
                        let diff = (p[ch] - expected[ch]).abs();
                        h_far_sq[ch] += diff * diff;
                        if diff > h_far_max[ch] {
                            h_far_max[ch] = diff;
                        }
                    }
                    h_far_cnt += 1;
                } else {
                    for ch in 0..3 {
                        let diff = (p[ch] - expected[ch]).abs();
                        h_near_sq[ch] += diff * diff;
                        if diff > h_near_max[ch] {
                            h_near_max[ch] = diff;
                        }
                    }
                    h_near_cnt += 1;
                }
            }
        }
        let h_far_rms = [
            (h_far_sq[0] / h_far_cnt as f32).sqrt(),
            (h_far_sq[1] / h_far_cnt as f32).sqrt(),
            (h_far_sq[2] / h_far_cnt as f32).sqrt(),
        ];
        let h_near_rms = [
            (h_near_sq[0] / h_near_cnt as f32).sqrt(),
            (h_near_sq[1] / h_near_cnt as f32).sqrt(),
            (h_near_sq[2] / h_near_cnt as f32).sqrt(),
        ];
        println!("Test 6 Horizontal far-field (>3px): RMS=[R:{:.6}, G:{:.6}, B:{:.6}], MAX=[R:{:.6}, G:{:.6}, B:{:.6}]",
            h_far_rms[0], h_far_rms[1], h_far_rms[2], h_far_max[0], h_far_max[1], h_far_max[2]);
        println!("Test 6 Horizontal near-edge (<=3px): RMS=[R:{:.6}, G:{:.6}, B:{:.6}], MAX=[R:{:.6}, G:{:.6}, B:{:.6}]",
            h_near_rms[0], h_near_rms[1], h_near_rms[2], h_near_max[0], h_near_max[1], h_near_max[2]);

        // 3. 45° diagonal step edge at r + c = 96
        let mut d_sensor = Vec::with_capacity(size * size);
        for r in 0..size {
            for c in 0..size {
                let ch = cfa.color_at(r, c);
                let col = if r + c < 96 { col1 } else { col2 };
                d_sensor.push(col[ch]);
            }
        }
        let res_d = demosaic(&PixF32::new_with(d_sensor, size, size), &cfa, roi);
        let mut d_far_max = [0.0f32; 3];
        let mut d_far_sq = [0.0f32; 3];
        let mut d_far_cnt = 0;
        let mut d_near_max = [0.0f32; 3];
        let mut d_near_sq = [0.0f32; 3];
        let mut d_near_cnt = 0;
        for r in 4..(size - 4) {
            for c in 4..(size - 4) {
                let dist_diag = ((r + c) as isize - 96).abs() as f32 / std::f32::consts::SQRT_2;
                let expected = if r + c < 96 { col1 } else { col2 };
                let p = res_d[r * size + c];
                if dist_diag > 3.0 {
                    for ch in 0..3 {
                        let diff = (p[ch] - expected[ch]).abs();
                        d_far_sq[ch] += diff * diff;
                        if diff > d_far_max[ch] {
                            d_far_max[ch] = diff;
                        }
                    }
                    d_far_cnt += 1;
                } else {
                    for ch in 0..3 {
                        let diff = (p[ch] - expected[ch]).abs();
                        d_near_sq[ch] += diff * diff;
                        if diff > d_near_max[ch] {
                            d_near_max[ch] = diff;
                        }
                    }
                    d_near_cnt += 1;
                }
            }
        }
        let d_far_rms = [
            (d_far_sq[0] / d_far_cnt as f32).sqrt(),
            (d_far_sq[1] / d_far_cnt as f32).sqrt(),
            (d_far_sq[2] / d_far_cnt as f32).sqrt(),
        ];
        let d_near_rms = [
            (d_near_sq[0] / d_near_cnt as f32).sqrt(),
            (d_near_sq[1] / d_near_cnt as f32).sqrt(),
            (d_near_sq[2] / d_near_cnt as f32).sqrt(),
        ];
        println!("Test 6 Diagonal far-field (>3px): RMS=[R:{:.6}, G:{:.6}, B:{:.6}], MAX=[R:{:.6}, G:{:.6}, B:{:.6}]",
            d_far_rms[0], d_far_rms[1], d_far_rms[2], d_far_max[0], d_far_max[1], d_far_max[2]);
        println!("Test 6 Diagonal near-edge (<=3px): RMS=[R:{:.6}, G:{:.6}, B:{:.6}], MAX=[R:{:.6}, G:{:.6}, B:{:.6}]",
            d_near_rms[0], d_near_rms[1], d_near_rms[2], d_near_max[0], d_near_max[1], d_near_max[2]);

        // Assertions for Test 6:
        // Far field <= 0.002
        for ch in 0..3 {
            assert!(
                v_far_max[ch] <= 0.002,
                "Vertical step far-field ch {} max = {} > 0.002",
                ch,
                v_far_max[ch]
            );
            assert!(
                h_far_max[ch] <= 0.002,
                "Horizontal step far-field ch {} max = {} > 0.002",
                ch,
                h_far_max[ch]
            );
            assert!(
                d_far_max[ch] <= 0.002,
                "Diagonal step far-field ch {} max = {} > 0.002",
                ch,
                d_far_max[ch]
            );
        }
        // Near edge <= 0.10
        for ch in 0..3 {
            assert!(
                v_near_max[ch] <= 0.10,
                "Vertical step near-edge ch {} max = {} > 0.10",
                ch,
                v_near_max[ch]
            );
            assert!(
                h_near_max[ch] <= 0.10,
                "Horizontal step near-edge ch {} max = {} > 0.10",
                ch,
                h_near_max[ch]
            );
            assert!(
                d_near_max[ch] <= 0.10,
                "Diagonal step near-edge ch {} max = {} > 0.10",
                ch,
                d_near_max[ch]
            );
        }
    }
}
