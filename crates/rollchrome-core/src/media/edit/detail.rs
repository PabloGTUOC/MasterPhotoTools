//! Sharpening, luminance and colour noise reduction, and hot pixel removal (RP-7).
//!
//! Evaluated on the stored frame before geometry, run once on the source proxy and cached
//! in `PreviewSession` so that colour and tonal adjustments cost nothing more.
//! Box filters run in O(1) via separable sliding sums across rows and columns.

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use super::pipeline::LinearBuffer;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Detail {
    /// Sharpening strength: 0..150, default 0.
    #[serde(default = "default_sharpen_amount")]
    pub sharpen_amount: f32,
    /// Sharpening radius in source pixels: 0.5..3.0, default 1.0.
    #[serde(default = "default_sharpen_radius")]
    pub sharpen_radius: f32,
    /// Edge masking threshold: 0..100, default 0 (unmasked).
    #[serde(default = "default_sharpen_masking")]
    pub sharpen_masking: f32,
    /// Luminance noise reduction: 0..100, default 0.
    #[serde(default = "default_noise_luminance")]
    pub noise_luminance: f32,
    /// Colour noise reduction: 0..100, default 0.
    #[serde(default = "default_noise_colour")]
    pub noise_colour: f32,
    /// Hot pixel detection and suppression: default false.
    #[serde(default)]
    pub hot_pixels: bool,
    /// Clarity: -100..100, default 0.
    #[serde(default)]
    pub clarity: f32,
    /// Texture: -100..100, default 0.
    #[serde(default)]
    pub texture: f32,
    /// Dehaze: -100..100, default 0.
    #[serde(default)]
    pub dehaze: f32,
}

fn default_sharpen_amount() -> f32 {
    0.0
}
fn default_sharpen_radius() -> f32 {
    1.0
}
fn default_sharpen_masking() -> f32 {
    0.0
}
fn default_noise_luminance() -> f32 {
    0.0
}
fn default_noise_colour() -> f32 {
    0.0
}

impl Default for Detail {
    fn default() -> Self {
        Self {
            sharpen_amount: 0.0,
            sharpen_radius: 1.0,
            sharpen_masking: 0.0,
            noise_luminance: 0.0,
            noise_colour: 0.0,
            hot_pixels: false,
            clarity: 0.0,
            texture: 0.0,
            dehaze: 0.0,
        }
    }
}

impl Detail {
    /// Returns true when all adjustments are at their neutral non-acting values.
    pub fn is_identity(&self) -> bool {
        self.sharpen_amount == 0.0
            && self.noise_luminance == 0.0
            && self.noise_colour == 0.0
            && !self.hot_pixels
            && self.clarity == 0.0
            && self.texture == 0.0
            && self.dehaze == 0.0
    }
}

/// Combines a global detail recipe with a mask's local detail (RP-11b).
/// Clarity, texture and dehaze add and clamp to -100..100.
/// Sharpen amount gains sharpness * 1.5 and clamps to 0..150.
/// Radius, masking, noise reduction and hot pixels stay as in global Detail.
pub fn combine_detail(global: Option<&Detail>, local: &super::masks::LocalDetail) -> Detail {
    let mut d = global.cloned().unwrap_or_default();
    d.clarity = (d.clarity + local.clarity).clamp(-100.0, 100.0);
    d.texture = (d.texture + local.texture).clamp(-100.0, 100.0);
    d.dehaze = (d.dehaze + local.dehaze).clamp(-100.0, 100.0);
    d.sharpen_amount = (d.sharpen_amount + local.sharpness.max(0.0) * 1.5).clamp(0.0, 150.0);
    d
}

/// Applies detail processing (hot pixels, noise reduction, sharpening) to a linear buffer.
/// `scale` is this buffer's long edge over the source's: 1.0 for an export.
pub fn apply_detail(src: &LinearBuffer, detail: &Detail, scale: f32) -> LinearBuffer {
    if detail.is_identity() || src.width == 0 || src.height == 0 {
        return src.clone();
    }

    let w = src.width as usize;
    let h = src.height as usize;
    let n_pixels = w * h;

    // -------------------------------------------------------------------------
    // 1. Hot pixels in linear light
    // -------------------------------------------------------------------------
    let linear_data = if detail.hot_pixels {
        remove_hot_pixels(&src.data, w, h)
    } else {
        src.data.clone()
    };

    // -------------------------------------------------------------------------
    // 2. Convert to sqrt of linear (noise uniform encoding), then Y, Cb, Cr
    // -------------------------------------------------------------------------
    let mut y_plane = vec![0.0f32; n_pixels];
    let mut cb_plane = vec![0.0f32; n_pixels];
    let mut cr_plane = vec![0.0f32; n_pixels];

    y_plane
        .par_chunks_mut(w)
        .zip(cb_plane.par_chunks_mut(w))
        .zip(cr_plane.par_chunks_mut(w))
        .zip(linear_data.par_chunks(w * 3))
        .for_each(|(((y_row, cb_row), cr_row), rgb_row)| {
            for x in 0..w {
                let r = rgb_row[x * 3].max(0.0).sqrt();
                let g = rgb_row[x * 3 + 1].max(0.0).sqrt();
                let b = rgb_row[x * 3 + 2].max(0.0).sqrt();

                let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
                y_row[x] = y;
                cb_row[x] = b - y;
                cr_row[x] = r - y;
            }
        });

    let scale_eff = scale.max(0.01);
    let working_l = w.max(h) as f32;

    // -------------------------------------------------------------------------
    // 3. Colour noise reduction (guided filter on Cb and Cr with Y as guide)
    // -------------------------------------------------------------------------
    if detail.noise_colour > 0.0 {
        let r_chroma = (8.0 * scale_eff).round().max(1.0) as usize;
        // Regularisation constant: scaled so amount 50 provides ~0.005, which attenuates
        // uncorrelated chroma noise variance by >75% while keeping luminance structure.
        let s = detail.noise_colour / 100.0;
        let eps_colour = s * s * 0.02;

        cb_plane = guided_filter(&y_plane, &cb_plane, w, h, r_chroma, eps_colour);
        cr_plane = guided_filter(&y_plane, &cr_plane, w, h, r_chroma, eps_colour);
    }

    // -------------------------------------------------------------------------
    // 4. Luminance noise reduction (guided filter of Y by itself)
    // -------------------------------------------------------------------------
    if detail.noise_luminance > 0.0 {
        let r_luma = (2.0 * scale_eff).round().max(1.0) as usize;
        // Regularisation constant: chosen so at amount 50 (s=0.5), eps is 0.004.
        // In flat areas, variance ~0.0025 gives a <= 0.41, halving noise std deviation.
        // At edges, step variance ~0.09 gives a >= 0.95, retaining >90% step height.
        let s = detail.noise_luminance / 100.0;
        let eps_luma = s * s * 0.016;

        y_plane = guided_filter_self(&y_plane, w, h, r_luma, eps_luma);
    }

    // -------------------------------------------------------------------------
    // 5. Clarity (guided filter on Y plane, midtone contrast)
    // -------------------------------------------------------------------------
    if detail.clarity != 0.0 {
        let r_clarity = (0.01 * working_l).round().max(1.0) as usize;
        let eps_clarity = 0.002f32;
        let base = guided_filter_self(&y_plane, w, h, r_clarity, eps_clarity);
        let factor = detail.clarity / 100.0;

        y_plane
            .par_iter_mut()
            .zip(base.par_iter())
            .for_each(|(y, &b)| {
                let orig = *y;
                let d = orig - b;
                let t = 2.0 * orig - 1.0;
                let w_mask = (1.0 - t * t).clamp(0.0, 1.0);
                *y = (orig + factor * d * w_mask).max(0.0);
            });
    }

    // -------------------------------------------------------------------------
    // 6. Texture (bandpass on Y plane)
    // -------------------------------------------------------------------------
    if detail.texture != 0.0 {
        let sigma_1 = (0.0004 * working_l).max(0.5);
        let sigma_2 = 0.003 * working_l;

        let g1 = gaussian_blur_2d(&y_plane, w, h, sigma_1);
        let g2 = if sigma_2 < 3.0 {
            gaussian_blur_2d(&y_plane, w, h, sigma_2)
        } else {
            let r_box = (((1.0 + 4.0 * sigma_2 * sigma_2).sqrt() - 1.0) / 2.0)
                .round()
                .max(1.0) as usize;
            let pass1 = box_filter_2d(&y_plane, w, h, r_box);
            let pass2 = box_filter_2d(&pass1, w, h, r_box);
            box_filter_2d(&pass2, w, h, r_box)
        };

        let factor = (detail.texture / 100.0) * 1.5;
        y_plane
            .par_iter_mut()
            .zip(g1.par_iter())
            .zip(g2.par_iter())
            .for_each(|((y, &v1), &v2)| {
                let band = v1 - v2;
                *y = (*y + factor * band).max(0.0);
            });
    }

    // -------------------------------------------------------------------------
    // 7. Sharpening: D = Y - gaussian(Y), real separable Gaussian blur with sigma
    // -------------------------------------------------------------------------
    if detail.sharpen_amount > 0.0 {
        let sigma = (detail.sharpen_radius * scale_eff).max(0.35);
        let blurred_y = gaussian_blur_2d(&y_plane, w, h, sigma);

        let amount_factor = detail.sharpen_amount / 100.0;
        let has_masking = detail.sharpen_masking > 0.0;
        let t = (detail.sharpen_masking / 100.0) * 0.08;

        y_plane
            .par_chunks_mut(w)
            .enumerate()
            .for_each(|(y_idx, y_row)| {
                for x in 0..w {
                    let y_orig = y_row[x];
                    let y_blur = blurred_y[y_idx * w + x];
                    let d = y_orig - y_blur;

                    let mask = if has_masking {
                        // Central difference gradient on blurred Y
                        let x_prev = if x > 0 { x - 1 } else { 0 };
                        let x_next = if x + 1 < w { x + 1 } else { w - 1 };
                        let gx =
                            (blurred_y[y_idx * w + x_next] - blurred_y[y_idx * w + x_prev]) * 0.5;

                        let y_prev = if y_idx > 0 { y_idx - 1 } else { 0 };
                        let y_next = if y_idx + 1 < h { y_idx + 1 } else { h - 1 };
                        let gy = (blurred_y[y_next * w + x] - blurred_y[y_prev * w + x]) * 0.5;

                        let grad = (gx * gx + gy * gy).sqrt();
                        smoothstep(0.5 * t, 1.5 * t, grad)
                    } else {
                        1.0
                    };

                    y_row[x] = y_orig + amount_factor * d * mask;
                }
            });
    }

    // -------------------------------------------------------------------------
    // 8. Rebuild linear RGB: R = Y + Cr, B = Y + Cb, G = (Y - 0.2126R - 0.0722B)/0.7152
    // -------------------------------------------------------------------------
    let mut out_data = vec![0.0f32; n_pixels * 3];

    out_data
        .par_chunks_mut(w * 3)
        .zip(y_plane.par_chunks(w))
        .zip(cb_plane.par_chunks(w))
        .zip(cr_plane.par_chunks(w))
        .for_each(|(((rgb_row, y_row), cb_row), cr_row)| {
            for x in 0..w {
                let y = y_row[x];
                let cb = cb_row[x];
                let cr = cr_row[x];

                let r = y + cr;
                let b = y + cb;
                let g = if cb == 0.0 && cr == 0.0 {
                    y
                } else {
                    (y - 0.2126 * r - 0.0722 * b) / 0.7152
                };

                let r_clamped = r.max(0.0);
                let g_clamped = g.max(0.0);
                let b_clamped = b.max(0.0);

                rgb_row[x * 3] = r_clamped * r_clamped;
                rgb_row[x * 3 + 1] = g_clamped * g_clamped;
                rgb_row[x * 3 + 2] = b_clamped * b_clamped;
            }
        });

    // -------------------------------------------------------------------------
    // 9. Dehaze in linear RGB
    // -------------------------------------------------------------------------
    if detail.dehaze != 0.0 {
        apply_dehaze(&mut out_data, w, h, detail.dehaze);
    }

    LinearBuffer {
        width: src.width,
        height: src.height,
        data: out_data,
    }
}

/// Hot pixel suppression in linear light.
/// Replaces any channel value exceeding `4 * max(neighbours) + 0.02` with the median of neighbours.
fn remove_hot_pixels(src: &[f32], w: usize, h: usize) -> Vec<f32> {
    let mut out = src.to_vec();

    out.par_chunks_mut(w * 3)
        .enumerate()
        .for_each(|(y, out_row)| {
            let mut neighbours = [0.0f32; 8];

            for x in 0..w {
                for c in 0..3 {
                    let val = src[(y * w + x) * 3 + c];
                    let mut count = 0;

                    for dy in -1i32..=1i32 {
                        for dx in -1i32..=1i32 {
                            if dx == 0 && dy == 0 {
                                continue;
                            }
                            let nx = x as i32 + dx;
                            let ny = y as i32 + dy;
                            if nx >= 0 && nx < w as i32 && ny >= 0 && ny < h as i32 {
                                neighbours[count] = src[(ny as usize * w + nx as usize) * 3 + c];
                                count += 1;
                            }
                        }
                    }

                    if count > 0 {
                        let max_n = neighbours[..count]
                            .iter()
                            .copied()
                            .fold(f32::NEG_INFINITY, f32::max);

                        if val > 4.0 * max_n + 0.02 {
                            neighbours[..count].sort_by(|a, b| a.total_cmp(b));
                            let median = neighbours[count / 2];
                            out_row[x * 3 + c] = median;
                        }
                    }
                }
            }
        });

    out
}

/// Cubic smoothstep function for soft thresholding.
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    if edge1 <= edge0 {
        if x >= edge0 {
            return 1.0;
        } else {
            return 0.0;
        }
    }
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Separable Gaussian blur with a normalized kernel of radius `ceil(3 * sigma)`.
/// `sigma` is at most 3.0, so the kernel is small and direct 1D convolution
/// is fast, exact, and scales smoothly down to sub-pixel radii.
#[allow(clippy::needless_range_loop)]
fn gaussian_blur_2d(src: &[f32], w: usize, h: usize, sigma: f32) -> Vec<f32> {
    if w == 0 || h == 0 {
        return src.to_vec();
    }
    let r = (3.0 * sigma).ceil().max(1.0) as usize;
    let mut kernel = Vec::with_capacity(2 * r + 1);
    let two_sigma_sq = 2.0 * sigma * sigma;
    let mut sum = 0.0f32;
    for k in -(r as isize)..=(r as isize) {
        let weight = (-(k * k) as f32 / two_sigma_sq).exp();
        kernel.push(weight);
        sum += weight;
    }
    let inv_sum = 1.0 / sum;
    for w_val in &mut kernel {
        *w_val *= inv_sum;
    }

    // Horizontal pass: src -> tmp
    let mut tmp = vec![0.0f32; w * h];
    tmp.par_chunks_mut(w).enumerate().for_each(|(y, tmp_row)| {
        let src_row = &src[y * w..(y + 1) * w];
        for x in 0..w {
            let mut acc = 0.0f32;
            for (k_idx, &weight) in kernel.iter().enumerate() {
                let k = k_idx as isize - r as isize;
                let nx = (x as isize + k).clamp(0, (w - 1) as isize) as usize;
                acc += src_row[nx] * weight;
            }
            tmp_row[x] = acc;
        }
    });

    // Vertical pass: tmp -> dst
    let mut dst = vec![0.0f32; w * h];
    dst.par_chunks_mut(w).enumerate().for_each(|(y, dst_row)| {
        for (k_idx, &weight) in kernel.iter().enumerate() {
            let k = k_idx as isize - r as isize;
            let ny = (y as isize + k).clamp(0, (h - 1) as isize) as usize;
            let tmp_row = &tmp[ny * w..(ny + 1) * w];
            for x in 0..w {
                dst_row[x] += tmp_row[x] * weight;
            }
        }
    });

    dst
}

/// 2D box filter by separable sliding sum with edge replication. O(1) per pixel.
#[allow(clippy::needless_range_loop)]
pub fn box_filter_2d(src: &[f32], width: usize, height: usize, r: usize) -> Vec<f32> {
    if r == 0 || width == 0 || height == 0 {
        return src.to_vec();
    }

    let mut temp = vec![0.0f32; width * height];
    let mut out = vec![0.0f32; width * height];

    // Horizontal pass: parallelized over rows
    let win_w = (2 * r + 1) as f32;
    let inv_win_w = 1.0 / win_w;

    temp.par_chunks_mut(width)
        .zip(src.par_chunks(width))
        .for_each(|(out_row, in_row)| {
            let mut sum = (r + 1) as f32 * in_row[0];
            for i in 1..=r {
                sum += in_row[i.min(width - 1)];
            }
            out_row[0] = sum * inv_win_w;

            for x in 1..width {
                let add_idx = (x + r).min(width - 1);
                let sub_idx = if x > r { x - 1 - r } else { 0 };
                sum += in_row[add_idx] - in_row[sub_idx];
                out_row[x] = sum * inv_win_w;
            }
        });

    // Transpose temp (W x H -> H x W)
    let mut trans = vec![0.0f32; width * height];
    transpose(&temp, &mut trans, width, height);

    // Vertical pass: horizontal sliding sum on transposed buffer
    let mut trans_filtered = vec![0.0f32; width * height];
    let win_h = (2 * r + 1) as f32;
    let inv_win_h = 1.0 / win_h;

    trans_filtered
        .par_chunks_mut(height)
        .zip(trans.par_chunks(height))
        .for_each(|(out_row, in_row)| {
            let mut sum = (r + 1) as f32 * in_row[0];
            for i in 1..=r {
                sum += in_row[i.min(height - 1)];
            }
            out_row[0] = sum * inv_win_h;

            for y in 1..height {
                let add_idx = (y + r).min(height - 1);
                let sub_idx = if y > r { y - 1 - r } else { 0 };
                sum += in_row[add_idx] - in_row[sub_idx];
                out_row[y] = sum * inv_win_h;
            }
        });

    // Transpose back (H x W -> W x H)
    transpose(&trans_filtered, &mut out, height, width);

    out
}

/// Cache-blocked matrix transpose.
fn transpose(src: &[f32], dst: &mut [f32], w: usize, h: usize) {
    const BLOCK: usize = 32;

    dst.par_chunks_mut(h * BLOCK.min(w))
        .enumerate()
        .for_each(|(chunk_idx, chunk)| {
            let x_start = chunk_idx * BLOCK;
            let num_x = chunk.len() / h;

            for dx in 0..num_x {
                let x = x_start + dx;
                for y in 0..h {
                    chunk[dx * h + y] = src[y * w + x];
                }
            }
        });
}

/// Guided filter of `p` using guide `guide`.
fn guided_filter(guide: &[f32], p: &[f32], w: usize, h: usize, r: usize, eps: f32) -> Vec<f32> {
    let n = w * h;

    let mean_i = box_filter_2d(guide, w, h, r);
    let mean_p = box_filter_2d(p, w, h, r);

    let mut i_sq = vec![0.0f32; n];
    let mut i_p = vec![0.0f32; n];

    i_sq.par_iter_mut()
        .zip(i_p.par_iter_mut())
        .zip(guide.par_iter())
        .zip(p.par_iter())
        .for_each(|(((isq, ip), &i_val), &p_val)| {
            *isq = i_val * i_val;
            *ip = i_val * p_val;
        });

    let corr_i = box_filter_2d(&i_sq, w, h, r);
    let corr_ip = box_filter_2d(&i_p, w, h, r);

    let mut a = vec![0.0f32; n];
    let mut b = vec![0.0f32; n];

    a.par_iter_mut()
        .zip(b.par_iter_mut())
        .enumerate()
        .for_each(|(idx, (a_val, b_val))| {
            let mi = mean_i[idx];
            let mp = mean_p[idx];
            let var_i = (corr_i[idx] - mi * mi).max(0.0);
            let cov_ip = corr_ip[idx] - mi * mp;

            let coeff_a = cov_ip / (var_i + eps);
            let coeff_b = mp - coeff_a * mi;

            *a_val = coeff_a;
            *b_val = coeff_b;
        });

    let mean_a = box_filter_2d(&a, w, h, r);
    let mean_b = box_filter_2d(&b, w, h, r);

    let mut q = vec![0.0f32; n];
    q.par_iter_mut().enumerate().for_each(|(idx, q_val)| {
        *q_val = mean_a[idx] * guide[idx] + mean_b[idx];
    });

    q
}

/// Guided filter of `guide` by itself (guide == p).
fn guided_filter_self(guide: &[f32], w: usize, h: usize, r: usize, eps: f32) -> Vec<f32> {
    let n = w * h;

    let mean_i = box_filter_2d(guide, w, h, r);

    let mut i_sq = vec![0.0f32; n];
    i_sq.par_iter_mut()
        .zip(guide.par_iter())
        .for_each(|(isq, &i_val)| {
            *isq = i_val * i_val;
        });

    let corr_i = box_filter_2d(&i_sq, w, h, r);

    let mut a = vec![0.0f32; n];
    let mut b = vec![0.0f32; n];

    a.par_iter_mut()
        .zip(b.par_iter_mut())
        .enumerate()
        .for_each(|(idx, (a_val, b_val))| {
            let mi = mean_i[idx];
            let var_i = (corr_i[idx] - mi * mi).max(0.0);

            let coeff_a = var_i / (var_i + eps);
            let coeff_b = (1.0 - coeff_a) * mi;

            *a_val = coeff_a;
            *b_val = coeff_b;
        });

    let mean_a = box_filter_2d(&a, w, h, r);
    let mean_b = box_filter_2d(&b, w, h, r);

    let mut q = vec![0.0f32; n];
    q.par_iter_mut().enumerate().for_each(|(idx, q_val)| {
        *q_val = mean_a[idx] * guide[idx] + mean_b[idx];
    });

    q
}

/// Applies dehaze in linear RGB space using an adapted Dark Channel Prior.
fn apply_dehaze(rgb_data: &mut [f32], w: usize, h: usize, dehaze: f32) {
    let working_l = (w.max(h) as f32).max(1.0);
    let s_down = if working_l > 1024.0 {
        1024.0 / working_l
    } else {
        1.0
    };
    let cw = ((w as f32) * s_down).round().max(1.0) as usize;
    let ch = ((h as f32) * s_down).round().max(1.0) as usize;
    let copy_l = (cw.max(ch) as f32).max(1.0);

    let copy_rgb = if cw == w && ch == h {
        rgb_data.to_vec()
    } else {
        downsample_bilinear_rgb(rgb_data, w, h, cw, ch)
    };

    let n_copy = cw * ch;
    let mut min_rgb = vec![0.0f32; n_copy];
    min_rgb.par_chunks_mut(cw).enumerate().for_each(|(y, row)| {
        for x in 0..cw {
            let r = copy_rgb[(y * cw + x) * 3];
            let g = copy_rgb[(y * cw + x) * 3 + 1];
            let b = copy_rgb[(y * cw + x) * 3 + 2];
            row[x] = r.min(g).min(b);
        }
    });

    let r_dark = (0.015 * copy_l).round().max(1.0) as usize;
    let dark_channel = min_filter_2d(&min_rgb, cw, ch, r_dark);

    // Atmospheric light vector A from top 0.1% brightest in dark_channel
    let k = ((n_copy as f32) * 0.001).ceil().max(1.0) as usize;
    let k = k.min(n_copy);
    let mut indices: Vec<usize> = (0..n_copy).collect();
    indices.select_nth_unstable_by(n_copy - k, |&i, &j| {
        dark_channel[i].total_cmp(&dark_channel[j])
    });

    let top_k = &indices[n_copy - k..];
    let mut sum_r = 0.0f64;
    let mut sum_g = 0.0f64;
    let mut sum_b = 0.0f64;
    for &idx in top_k {
        sum_r += copy_rgb[idx * 3] as f64;
        sum_g += copy_rgb[idx * 3 + 1] as f64;
        sum_b += copy_rgb[idx * 3 + 2] as f64;
    }
    let inv_k = 1.0 / (top_k.len() as f64);
    let a_r = ((sum_r * inv_k) as f32).max(0.01);
    let a_g = ((sum_g * inv_k) as f32).max(0.01);
    let a_b = ((sum_b * inv_k) as f32).max(0.01);

    if dehaze > 0.0 {
        let omega = 0.95 * (dehaze / 100.0);
        let mut min_norm = vec![0.0f32; n_copy];
        min_norm
            .par_chunks_mut(cw)
            .enumerate()
            .for_each(|(y, row)| {
                for x in 0..cw {
                    let r = copy_rgb[(y * cw + x) * 3];
                    let g = copy_rgb[(y * cw + x) * 3 + 1];
                    let b = copy_rgb[(y * cw + x) * 3 + 2];
                    let norm = (r / a_r).min(g / a_g).min(b / a_b);
                    row[x] = norm;
                }
            });

        let dark_norm = min_filter_2d(&min_norm, cw, ch, r_dark);

        let mut t_raw = vec![0.0f32; n_copy];
        t_raw
            .par_iter_mut()
            .zip(dark_norm.par_iter())
            .for_each(|(t, &d)| {
                *t = 1.0 - omega * d;
            });

        let mut y_copy = vec![0.0f32; n_copy];
        y_copy.par_chunks_mut(cw).enumerate().for_each(|(y, row)| {
            for x in 0..cw {
                let r = copy_rgb[(y * cw + x) * 3].max(0.0);
                let g = copy_rgb[(y * cw + x) * 3 + 1].max(0.0);
                let b = copy_rgb[(y * cw + x) * 3 + 2].max(0.0);
                row[x] = (0.2126 * r + 0.7152 * g + 0.0722 * b).clamp(0.0, 1.0);
            }
        });

        let r_guide = (4 * r_dark).min(cw.max(ch) / 2).max(1);
        let t_refined = guided_filter(&y_copy, &t_raw, cw, ch, r_guide, 0.001);

        let mut t_clamped = vec![0.0f32; n_copy];
        t_clamped
            .par_iter_mut()
            .zip(t_refined.par_iter())
            .for_each(|(out, &t)| {
                *out = t.clamp(0.1, 1.0);
            });

        let t_full = if cw == w && ch == h {
            t_clamped
        } else {
            upsample_bilinear_1d(&t_clamped, cw, ch, w, h)
        };

        rgb_data
            .par_chunks_mut(w * 3)
            .zip(t_full.par_chunks(w))
            .for_each(|(row, t_row)| {
                for x in 0..w {
                    let t_val = t_row[x].max(0.1);
                    let inv_t = 1.0 / t_val;
                    let r = row[x * 3];
                    let g = row[x * 3 + 1];
                    let b = row[x * 3 + 2];
                    row[x * 3] = ((r - a_r) * inv_t + a_r).max(0.0);
                    row[x * 3 + 1] = ((g - a_g) * inv_t + a_g).max(0.0);
                    row[x * 3 + 2] = ((b - a_b) * inv_t + a_b).max(0.0);
                }
            });
    } else {
        // Negative dehaze: blend linearly toward atmospheric light A
        let h_veil = (dehaze.abs() / 100.0) * 0.6;
        let one_minus_h = 1.0 - h_veil;

        rgb_data.par_chunks_mut(w * 3).for_each(|row| {
            for x in 0..w {
                let r = row[x * 3];
                let g = row[x * 3 + 1];
                let b = row[x * 3 + 2];
                row[x * 3] = (r * one_minus_h + a_r * h_veil).max(0.0);
                row[x * 3 + 1] = (g * one_minus_h + a_g * h_veil).max(0.0);
                row[x * 3 + 2] = (b * one_minus_h + a_b * h_veil).max(0.0);
            }
        });
    }
}

/// 1D min filter across a contiguous slice with window `[max(0, x - r), min(len - 1, x + r)]`.
/// Runs in O(N) using a monotonic double-ended queue.
fn min_filter_1d(src: &[f32], r: usize) -> Vec<f32> {
    let len = src.len();
    if len == 0 || r == 0 {
        return src.to_vec();
    }
    let mut out = vec![0.0f32; len];
    let mut deque: std::collections::VecDeque<usize> =
        std::collections::VecDeque::with_capacity(2 * r + 2);

    let initial_end = r.min(len - 1);
    for i in 0..=initial_end {
        while let Some(&back) = deque.back() {
            if src[back] >= src[i] {
                deque.pop_back();
            } else {
                break;
            }
        }
        deque.push_back(i);
    }

    for (x, out_val) in out.iter_mut().enumerate() {
        let new_idx = x + r;
        if new_idx < len && new_idx > initial_end {
            while let Some(&back) = deque.back() {
                if src[back] >= src[new_idx] {
                    deque.pop_back();
                } else {
                    break;
                }
            }
            deque.push_back(new_idx);
        }
        let min_valid_idx = x.saturating_sub(r);
        while let Some(&front) = deque.front() {
            if front < min_valid_idx {
                deque.pop_front();
            } else {
                break;
            }
        }
        *out_val = src[*deque.front().unwrap()];
    }

    out
}

/// Separable 2D min filter across rows then columns in O(1) per pixel.
pub fn min_filter_2d(src: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    if r == 0 || w == 0 || h == 0 {
        return src.to_vec();
    }

    let mut temp = vec![0.0f32; w * h];
    temp.par_chunks_mut(w)
        .zip(src.par_chunks(w))
        .for_each(|(out_row, in_row)| {
            let filtered = min_filter_1d(in_row, r);
            out_row.copy_from_slice(&filtered);
        });

    let mut trans = vec![0.0f32; w * h];
    transpose(&temp, &mut trans, w, h);

    let mut trans_filtered = vec![0.0f32; w * h];
    trans_filtered
        .par_chunks_mut(h)
        .zip(trans.par_chunks(h))
        .for_each(|(out_row, in_row)| {
            let filtered = min_filter_1d(in_row, r);
            out_row.copy_from_slice(&filtered);
        });

    let mut out = vec![0.0f32; w * h];
    transpose(&trans_filtered, &mut out, h, w);

    out
}

/// Bilinear downsampling for RGB buffers.
fn downsample_bilinear_rgb(src: &[f32], w: usize, h: usize, cw: usize, ch: usize) -> Vec<f32> {
    if w == cw && h == ch {
        return src.to_vec();
    }
    let mut dst = vec![0.0f32; cw * ch * 3];
    let sx = w as f32 / cw as f32;
    let sy = h as f32 / ch as f32;

    dst.par_chunks_mut(cw * 3)
        .enumerate()
        .for_each(|(cy, row)| {
            let v = (cy as f32 + 0.5) * sy - 0.5;
            let y0 = (v.floor() as isize).clamp(0, h as isize - 1) as usize;
            let y1 = (y0 + 1).min(h - 1);
            let fy = (v - v.floor()).clamp(0.0, 1.0);

            for cx in 0..cw {
                let u = (cx as f32 + 0.5) * sx - 0.5;
                let x0 = (u.floor() as isize).clamp(0, w as isize - 1) as usize;
                let x1 = (x0 + 1).min(w - 1);
                let fx = (u - u.floor()).clamp(0.0, 1.0);

                let w00 = (1.0 - fx) * (1.0 - fy);
                let w10 = fx * (1.0 - fy);
                let w01 = (1.0 - fx) * fy;
                let w11 = fx * fy;

                for c in 0..3 {
                    let c00 = src[(y0 * w + x0) * 3 + c];
                    let c10 = src[(y0 * w + x1) * 3 + c];
                    let c01 = src[(y1 * w + x0) * 3 + c];
                    let c11 = src[(y1 * w + x1) * 3 + c];
                    row[cx * 3 + c] = w00 * c00 + w10 * c10 + w01 * c01 + w11 * c11;
                }
            }
        });

    dst
}

/// Bilinear upsampling for single-channel float buffers.
fn upsample_bilinear_1d(src: &[f32], cw: usize, ch: usize, w: usize, h: usize) -> Vec<f32> {
    if cw == w && ch == h {
        return src.to_vec();
    }
    let mut dst = vec![0.0f32; w * h];
    let sx = cw as f32 / w as f32;
    let sy = ch as f32 / h as f32;

    dst.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        let v = (y as f32 + 0.5) * sy - 0.5;
        let y0 = (v.floor() as isize).clamp(0, ch as isize - 1) as usize;
        let y1 = (y0 + 1).min(ch - 1);
        let fy = (v - v.floor()).clamp(0.0, 1.0);

        for (x, out_val) in row.iter_mut().enumerate() {
            let u = (x as f32 + 0.5) * sx - 0.5;
            let x0 = (u.floor() as isize).clamp(0, cw as isize - 1) as usize;
            let x1 = (x0 + 1).min(cw - 1);
            let fx = (u - u.floor()).clamp(0.0, 1.0);

            let w00 = (1.0 - fx) * (1.0 - fy);
            let w10 = fx * (1.0 - fy);
            let w01 = (1.0 - fx) * fy;
            let w11 = fx * fy;

            let c00 = src[y0 * cw + x0];
            let c10 = src[y0 * cw + x1];
            let c01 = src[y1 * cw + x0];
            let c11 = src[y1 * cw + x1];
            *out_val = w00 * c00 + w10 * c10 + w01 * c01 + w11 * c11;
        }
    });

    dst
}
