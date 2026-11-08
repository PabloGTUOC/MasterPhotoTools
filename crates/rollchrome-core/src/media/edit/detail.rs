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
    }
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
    // 5. Sharpening: D = Y - gaussian(Y), real separable Gaussian blur with sigma
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
    // 6. Rebuild linear RGB: R = Y + Cr, B = Y + Cb, G = (Y - 0.2126R - 0.0722B)/0.7152
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
                let g = (y - 0.2126 * r - 0.0722 * b) / 0.7152;

                let r_clamped = r.max(0.0);
                let g_clamped = g.max(0.0);
                let b_clamped = b.max(0.0);

                rgb_row[x * 3] = r_clamped * r_clamped;
                rgb_row[x * 3 + 1] = g_clamped * g_clamped;
                rgb_row[x * 3 + 2] = b_clamped * b_clamped;
            }
        });

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
