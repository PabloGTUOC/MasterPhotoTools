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
//!    the nearest green samples within 3 pixels. Directions are ranked by a
//!    gradient (difference in green plus difference in same-colour samples).
//!    Estimates with gradients within 10% (or 1e-6) of the minimum gradient
//!    are averaged.
//! 2. Red and blue from colour differences: at photosites missing red or blue,
//!    colour differences (R - G or B - G) from the 5x5 neighbourhood are
//!    averaged with weights combining inverse distance and green similarity:
//!    w = 1/dist + 1 / (1 + 10 * |G_sample - G_centre|).
//!    The weighted mean difference is added back to G_centre.
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

    // Step 2: Red and Blue from colour differences
    let rgb_pixels: Vec<[f32; 3]> = (0..h)
        .into_par_iter()
        .flat_map(|r| {
            let mut row_rgb = Vec::with_capacity(w);
            let row_offset = r * w;
            for c in 0..w {
                let col_id = cfa_roi.color_at(r, c);
                let g = green_plane[row_offset + c];

                let r_val = if col_id == 0 {
                    get_sensor(r, c)
                } else {
                    estimate_chroma(r, c, w, h, 0, g, &green_plane, &cfa_roi, &get_sensor)
                };

                let b_val = if col_id == 2 {
                    get_sensor(r, c)
                } else {
                    estimate_chroma(r, c, w, h, 2, g, &green_plane, &cfa_roi, &get_sensor)
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
        // Nearest green on positive side within 3 pixels
        let mut pos_green = None;
        for k in 1..=3isize {
            let nr = ir + k * dr;
            let nc = ic + k * dc;
            if nr >= 0 && nr < ih && nc >= 0 && nc < iw {
                let ur = nr as usize;
                let uc = nc as usize;
                if cfa_roi.color_at(ur, uc) == 1 {
                    pos_green = Some((k, get_sensor(ur, uc)));
                    break;
                }
            }
        }

        // Nearest green on negative side within 3 pixels
        let mut neg_green = None;
        for k in 1..=3isize {
            let nr = ir - k * dr;
            let nc = ic - k * dc;
            if nr >= 0 && nr < ih && nc >= 0 && nc < iw {
                let ur = nr as usize;
                let uc = nc as usize;
                if cfa_roi.color_at(ur, uc) == 1 {
                    neg_green = Some((k, get_sensor(ur, uc)));
                    break;
                }
            }
        }

        if let (Some((kp, gp)), Some((kn, gn))) = (pos_green, neg_green) {
            // Linear interpolation between the two greens
            let total_k = (kp + kn) as f32;
            let est = (kn as f32 * gp + kp as f32 * gn) / total_k;

            // Nearest same-colour sample on positive side within 3 pixels
            let mut pos_same = None;
            for m in 1..=3isize {
                let nr = ir + m * dr;
                let nc = ic + m * dc;
                if nr >= 0 && nr < ih && nc >= 0 && nc < iw {
                    let ur = nr as usize;
                    let uc = nc as usize;
                    if cfa_roi.color_at(ur, uc) == centre_color {
                        pos_same = Some(get_sensor(ur, uc));
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
                        neg_same = Some(get_sensor(ur, uc));
                        break;
                    }
                }
            }

            let centre_val = get_sensor(r, c);
            let same_diff = match (pos_same, neg_same) {
                (Some(sp), Some(sn)) => (sp - sn).abs(),
                (Some(sp), None) => (sp - centre_val).abs(),
                (None, Some(sn)) => (sn - centre_val).abs(),
                (None, None) => 0.0,
            };

            let grad = (gp - gn).abs() + same_diff;
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
#[allow(clippy::too_many_arguments)]
fn estimate_chroma<F>(
    r: usize,
    c: usize,
    w: usize,
    h: usize,
    target_channel: usize,
    g_centre: f32,
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

    let mut sum_w = 0.0f32;
    let mut sum_w_diff = 0.0f32;

    for dr in -2..=2isize {
        for dc in -2..=2isize {
            if dr == 0 && dc == 0 {
                continue;
            }
            let nr = ir + dr;
            let nc = ic + dc;
            if nr >= 0 && nr < ih && nc >= 0 && nc < iw {
                let ur = nr as usize;
                let uc = nc as usize;
                if cfa_roi.color_at(ur, uc) == target_channel {
                    let dist = ((dr * dr + dc * dc) as f32).sqrt();
                    let g_sample = green_plane[ur * w + uc];
                    let weight = (1.0 / dist) + (1.0 / (1.0 + (g_sample - g_centre).abs() * 10.0));
                    let diff = get_sensor(ur, uc) - g_sample;
                    sum_w += weight;
                    sum_w_diff += weight * diff;
                }
            }
        }
    }

    if sum_w > 0.0 {
        g_centre + sum_w_diff / sum_w
    } else {
        // Fallback in case 5x5 boundary has no target sample: search 7x7
        for dr in -3..=3isize {
            for dc in -3..=3isize {
                let nr = ir + dr;
                let nc = ic + dc;
                if nr >= 0 && nr < ih && nc >= 0 && nc < iw {
                    let ur = nr as usize;
                    let uc = nc as usize;
                    if cfa_roi.color_at(ur, uc) == target_channel {
                        let dist = ((dr * dr + dc * dc) as f32).sqrt().max(1.0);
                        let g_sample = green_plane[ur * w + uc];
                        let weight = 1.0 / dist;
                        let diff = get_sensor(ur, uc) - g_sample;
                        sum_w += weight;
                        sum_w_diff += weight * diff;
                    }
                }
            }
        }
        if sum_w > 0.0 {
            g_centre + sum_w_diff / sum_w
        } else {
            g_centre
        }
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
        for r in 4..(size - 4) {
            for c in 4..(size - 4) {
                let expected = 0.3 + 0.2 * (2.0 * std::f32::consts::PI * c as f32 / period).sin();
                let p = res_h[r * size + c];
                for ch in 0..3 {
                    let d = p[ch] - expected;
                    sum_sq_h[ch] += d * d;
                }
                count_h += 1;
            }
        }
        let rms_h = [
            (sum_sq_h[0] / count_h as f32).sqrt(),
            (sum_sq_h[1] / count_h as f32).sqrt(),
            (sum_sq_h[2] / count_h as f32).sqrt(),
        ];
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
        for r in 4..(size - 4) {
            for c in 4..(size - 4) {
                let expected = 0.3 + 0.2 * (2.0 * std::f32::consts::PI * r as f32 / period).sin();
                let p = res_v[r * size + c];
                for ch in 0..3 {
                    let diff = p[ch] - expected;
                    sum_sq_v[ch] += diff * diff;
                }
                count_v += 1;
            }
        }
        let rms_v = [
            (sum_sq_v[0] / count_v as f32).sqrt(),
            (sum_sq_v[1] / count_v as f32).sqrt(),
            (sum_sq_v[2] / count_v as f32).sqrt(),
        ];
        for (ch, &val) in rms_v.iter().enumerate() {
            assert!(val <= 0.006, "90° grating RMS ch {} = {} > 0.006", ch, val);
        }
    }

    #[test]
    fn a_fine_diagonal_grating_comes_back_without_a_maze() {
        let size = 96;
        let cfa = xtrans_cfa();
        let period = 8.0f32;
        let roi = Rect::new(
            rawler::imgop::Point::new(0, 0),
            rawler::imgop::Dim2::new(size, size),
        );

        // 45° grating: coordinate (r + c) / sqrt(2), effective wavelength along diagonal
        let mut diag_sensor = Vec::with_capacity(size * size);
        for r in 0..size {
            for c in 0..size {
                let u = (r + c) as f32 / std::f32::consts::SQRT_2;
                let val = 0.3 + 0.2 * (2.0 * std::f32::consts::PI * u / period).sin();
                diag_sensor.push(val);
            }
        }
        let pix_diag = PixF32::new_with(diag_sensor, size, size);
        let res_diag = demosaic(&pix_diag, &cfa, roi);

        let mut sum_sq = [0.0f32; 3];
        let mut count = 0;
        for r in 4..(size - 4) {
            for c in 4..(size - 4) {
                let u = (r + c) as f32 / std::f32::consts::SQRT_2;
                let expected = 0.3 + 0.2 * (2.0 * std::f32::consts::PI * u / period).sin();
                let p = res_diag[r * size + c];
                for ch in 0..3 {
                    let diff = p[ch] - expected;
                    sum_sq[ch] += diff * diff;
                }
                count += 1;
            }
        }
        let rms = [
            (sum_sq[0] / count as f32).sqrt(),
            (sum_sq[1] / count as f32).sqrt(),
            (sum_sq[2] / count as f32).sqrt(),
        ];
        println!(
            "Diagonal grating RMS: R={:.6}, G={:.6}, B={:.6}",
            rms[0], rms[1], rms[2]
        );
        for (ch, &val) in rms.iter().enumerate() {
            assert!(
                val <= 0.010,
                "Diagonal grating RMS ch {} = {} > 0.010",
                ch,
                val
            );
        }
    }

    #[test]
    fn a_step_edge_leaves_the_far_field_untouched() {
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
        let mut max_err_v = 0.0f32;
        for r in 4..(size - 4) {
            for c in 4..(size - 4) {
                if (c as isize - 48).abs() > 3 {
                    let expected = if c < 48 { col1 } else { col2 };
                    let p = res_v[r * size + c];
                    for ch in 0..3 {
                        let diff = (p[ch] - expected[ch]).abs();
                        if diff > max_err_v {
                            max_err_v = diff;
                        }
                        assert!(
                            diff <= 0.002,
                            "Vertical step at ({},{}) ch {}: got {}, expected {}",
                            r,
                            c,
                            ch,
                            p[ch],
                            expected[ch]
                        );
                    }
                }
            }
        }
        println!("Vertical step max error: {:.6}", max_err_v);

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
        let mut max_err_h = 0.0f32;
        for r in 4..(size - 4) {
            for c in 4..(size - 4) {
                if (r as isize - 48).abs() > 3 {
                    let expected = if r < 48 { col1 } else { col2 };
                    let p = res_h[r * size + c];
                    for ch in 0..3 {
                        let diff = (p[ch] - expected[ch]).abs();
                        if diff > max_err_h {
                            max_err_h = diff;
                        }
                        assert!(
                            diff <= 0.002,
                            "Horizontal step at ({},{}) ch {}: got {}, expected {}",
                            r,
                            c,
                            ch,
                            p[ch],
                            expected[ch]
                        );
                    }
                }
            }
        }
        println!("Horizontal step max error: {:.6}", max_err_h);

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
        let mut max_err_d = 0.0f32;
        for r in 4..(size - 4) {
            for c in 4..(size - 4) {
                // Distance to line r + c = 96 is |r + c - 96| / sqrt(2)
                let dist_diag = ((r + c) as isize - 96).abs() as f32 / std::f32::consts::SQRT_2;
                if dist_diag > 3.0 {
                    let expected = if r + c < 96 { col1 } else { col2 };
                    let p = res_d[r * size + c];
                    for ch in 0..3 {
                        let diff = (p[ch] - expected[ch]).abs();
                        if diff > max_err_d {
                            max_err_d = diff;
                        }
                        assert!(
                            diff <= 0.002,
                            "Diagonal step at ({},{}) ch {}: got {}, expected {}",
                            r,
                            c,
                            ch,
                            p[ch],
                            expected[ch]
                        );
                    }
                }
            }
        }
        println!("Diagonal step max error: {:.6}", max_err_d);
    }
}
