//! Built-in look fitting, Levenberg-Marquardt optimizer, and 3D LUT baking (DV-3).

use image::RgbImage;
use phototools_core::error::Error;
use serde::{Deserialize, Serialize};

use crate::media::develop::colour as dev_colour;
use crate::media::develop::render::{render, Look, OutputFormat};
use crate::media::develop::tone;
use crate::media::develop::Developed;
use crate::media::edit::lut::Lut;
use crate::media::looks::colour as looks_colour;

/// Threshold constants from DV-3 brief.
pub const THRESHOLD_MEAN_DELTA_E: f32 = 2.5;
pub const THRESHOLD_MAX_DELTA_E: f32 = 7.0;
pub const THRESHOLD_LOO_MEAN_DELTA_E: f32 = 5.0;

/// Number of spline intervals (knots = K + 1).
pub const KNOTS_K: usize = 8;
/// Maximum value offset: y_K = 1 - EPSILON.
pub const EPSILON: f32 = 0.005;
/// Soft floor parameter.
pub const TAU: f32 = 1e-4;

/// Configuration loaded from `looks/fit.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FitConfig {
    pub source_repository: String,
    pub source_commit: String,
    pub cameras: Vec<CameraFitConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraFitConfig {
    pub camera: String,
    pub maker: String,
    pub raw: String,
    pub column_centres_x: Vec<u32>,
    pub row_centres_y: Vec<u32>,
    pub box_size: u32,
    pub looks: Vec<LookTargetConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LookTargetConfig {
    pub id: String,
    pub display_name: String,
    pub jpeg: String,
}

/// Stably computes softplus: ln(1 + e^x).
#[inline]
pub fn softplus(x: f32) -> f32 {
    if x > 30.0 {
        x
    } else if x < -30.0 {
        x.exp()
    } else {
        (1.0 + x.exp()).ln()
    }
}

/// Monotone cubic Hermite spline (Fritsch-Carlson) over [0, 1] with exponential shoulder above 1.
#[derive(Debug, Clone)]
pub struct HermiteCurve {
    pub k: usize,
    pub y_knots: Vec<f32>,
    pub tangents: Vec<f32>,
}

impl HermiteCurve {
    /// Builds curve from K parameter increments theta.
    pub fn from_theta(theta: &[f32]) -> Self {
        let k = theta.len();
        assert!(k >= 2);

        let p: Vec<f32> = theta.iter().map(|&t| softplus(t)).collect();
        let sum_p: f32 = p.iter().sum();
        let scale = if sum_p > 1e-7 {
            (1.0 - EPSILON) / sum_p
        } else {
            (1.0 - EPSILON) / k as f32
        };

        let mut y_knots = Vec::with_capacity(k + 1);
        y_knots.push(0.0);
        let mut cur = 0.0;
        for pi in &p {
            cur += pi * scale;
            y_knots.push(cur);
        }
        y_knots[k] = 1.0 - EPSILON;

        // Secant slopes: Delta_i = (y_{i+1} - y_i) / h, where h = 1/k.
        let h = 1.0 / k as f32;
        let mut deltas = Vec::with_capacity(k);
        for i in 0..k {
            deltas.push((y_knots[i + 1] - y_knots[i]) / h);
        }

        // Initial tangents
        let mut tangents = Vec::with_capacity(k + 1);
        tangents.push(deltas[0]);
        for i in 1..k {
            tangents.push(0.5 * (deltas[i - 1] + deltas[i]));
        }
        tangents.push(deltas[k - 1]);

        // Fritsch-Carlson monotonicity adjustment
        for i in 0..k {
            let delta = deltas[i];
            if delta.abs() < 1e-8 {
                tangents[i] = 0.0;
                tangents[i + 1] = 0.0;
            } else {
                let alpha = tangents[i] / delta;
                let beta = tangents[i + 1] / delta;
                let dist_sq = alpha * alpha + beta * beta;
                if dist_sq > 9.0 {
                    let tau = 3.0 / dist_sq.sqrt();
                    tangents[i] = tau * alpha * delta;
                    tangents[i + 1] = tau * beta * delta;
                }
            }
        }

        Self {
            k,
            y_knots,
            tangents,
        }
    }

    /// Evaluates curve at u.
    #[inline]
    pub fn eval(&self, u: f32) -> f32 {
        if u <= 0.0 {
            return 0.0;
        }
        if u > 1.0 {
            let d_k = self.tangents[self.k];
            return 1.0 - EPSILON * (-d_k * (u - 1.0) / EPSILON).exp();
        }

        let k_f = self.k as f32;
        let idx = (u * k_f).floor().clamp(0.0, (self.k - 1) as f32) as usize;
        let x_k = idx as f32 / k_f;
        let h = 1.0 / k_f;
        let t = ((u - x_k) / h).clamp(0.0, 1.0);

        let t2 = t * t;
        let t3 = t2 * t;

        let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
        let h10 = t3 - 2.0 * t2 + t;
        let h01 = -2.0 * t3 + 3.0 * t2;
        let h11 = t3 - t2;

        let val = h00 * self.y_knots[idx]
            + h10 * h * self.tangents[idx]
            + h01 * self.y_knots[idx + 1]
            + h11 * h * self.tangents[idx + 1];

        val.clamp(0.0, 1.0 - EPSILON)
    }
}

/// The parametric look model: gain g, 3x3 matrix M, and per-channel Hermite curves.
#[derive(Debug, Clone)]
pub struct LookModel {
    pub g: f32,
    pub m: [[f32; 3]; 3],
    pub curves: [HermiteCurve; 3],
}

impl LookModel {
    /// Builds model from parameter vector [M_9, theta_r, theta_g, theta_b].
    pub fn from_params(g: f32, p: &[f32], k: usize) -> Self {
        assert_eq!(p.len(), 9 + 3 * k);

        let m = [[p[0], p[1], p[2]], [p[3], p[4], p[5]], [p[6], p[7], p[8]]];

        let curve_r = HermiteCurve::from_theta(&p[9..9 + k]);
        let curve_g = HermiteCurve::from_theta(&p[9 + k..9 + 2 * k]);
        let curve_b = HermiteCurve::from_theta(&p[9 + 2 * k..9 + 3 * k]);

        Self {
            g,
            m,
            curves: [curve_r, curve_g, curve_b],
        }
    }

    /// Evaluates model on encoded ProPhoto RGB in [0, 1] directly.
    #[inline]
    pub fn eval_direct(&self, in_rgb: [f32; 3]) -> [f32; 3] {
        // Step 1: linear ProPhoto
        let x = [
            self.g * tone::prophoto_decode(in_rgb[0]),
            self.g * tone::prophoto_decode(in_rgb[1]),
            self.g * tone::prophoto_decode(in_rgb[2]),
        ];

        // Step 2: y = M * x
        let y = dev_colour::mul_3x3_vec(self.m, x);

        // Step 3: soft floor per channel
        let soft_floor = |val: f32| -> f32 {
            let z = val / TAU;
            if z > 40.0 {
                val
            } else if z < -40.0 {
                TAU * z.exp()
            } else {
                TAU * (1.0 + z.exp()).ln()
            }
        };

        let s = [soft_floor(y[0]), soft_floor(y[1]), soft_floor(y[2])];

        // Step 4: u = prophoto_encode(s)
        let u = [
            tone::prophoto_encode(s[0]),
            tone::prophoto_encode(s[1]),
            tone::prophoto_encode(s[2]),
        ];

        // Step 5: out_c = C_c(u)
        [
            self.curves[0].eval(u[0]),
            self.curves[1].eval(u[1]),
            self.curves[2].eval(u[2]),
        ]
    }

    /// Evaluates model on encoded ProPhoto RGB in [0, 1], guaranteeing strictly inside (0, 1)
    /// except black inputs which may be 0.
    #[inline]
    pub fn eval(&self, in_rgb: [f32; 3]) -> [f32; 3] {
        let raw = self.eval_direct(in_rgb);
        let clamp_out = |val: f32, in_val: f32| -> f32 {
            if val >= 1.0 {
                f32::from_bits(1.0f32.to_bits() - 1)
            } else if in_val > 0.0 && val <= 0.0 {
                f32::MIN_POSITIVE
            } else {
                val
            }
        };
        [
            clamp_out(raw[0], in_rgb[0]),
            clamp_out(raw[1], in_rgb[1]),
            clamp_out(raw[2], in_rgb[2]),
        ]
    }

    /// Computes determinant of matrix M.
    pub fn det_m(&self) -> f32 {
        let m = self.m;
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    }
}

/// Measures per-channel median and optionally spread (IQR) over a 120x120 box at (cx, cy).
/// Returns per-channel medians in [0, 1].
#[allow(clippy::too_many_arguments, clippy::manual_is_multiple_of)]
pub fn measure_patch_box(
    pixels_rgb: &[f32],
    width: usize,
    height: usize,
    cx: u32,
    cy: u32,
    box_size: u32,
    image_name: &str,
    patch_idx: usize,
    check_iqr: bool,
) -> Result<[f32; 3], Error> {
    let half = (box_size / 2) as usize;
    let cx = cx as usize;
    let cy = cy as usize;

    if cx < half || cx + half > width || cy < half || cy + half > height {
        return Err(Error::Refused(format!(
            "image '{image_name}' patch {patch_idx} box ({cx}, {cy}) exceeds image boundaries {width}x{height}"
        )));
    }

    let x0 = cx - half;
    let x1 = cx + half;
    let y0 = cy - half;
    let y1 = cy + half;

    let n = (x1 - x0) * (y1 - y0);
    let mut ch_r = Vec::with_capacity(n);
    let mut ch_g = Vec::with_capacity(n);
    let mut ch_b = Vec::with_capacity(n);

    for y in y0..y1 {
        let row_start = y * width * 3;
        for x in x0..x1 {
            let idx = row_start + x * 3;
            ch_r.push(pixels_rgb[idx]);
            ch_g.push(pixels_rgb[idx + 1]);
            ch_b.push(pixels_rgb[idx + 2]);
        }
    }

    let compute_stats = |mut v: Vec<f32>, ch_name: &str| -> Result<f32, Error> {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let median = if n % 2 == 0 {
            0.5 * (v[n / 2 - 1] + v[n / 2])
        } else {
            v[n / 2]
        };

        if check_iqr {
            let q1_idx = (n as f32 * 0.25).floor() as usize;
            let q3_idx = (n as f32 * 0.75).floor() as usize;
            let iqr = v[q3_idx] - v[q1_idx];

            if iqr > 0.04 {
                return Err(Error::Refused(format!(
                    "image '{image_name}' patch {patch_idx} channel {ch_name} has spread IQR {iqr:.4} > 0.04"
                )));
            }
        }

        Ok(median)
    };

    let med_r = compute_stats(ch_r, "R")?;
    let med_g = compute_stats(ch_g, "G")?;
    let med_b = compute_stats(ch_b, "B")?;

    Ok([med_r, med_g, med_b])
}

/// Measures all 24 patches from developed RAW image in encoded ProPhoto RGB.
pub fn measure_developed_patches(
    developed: &Developed,
    cam_cfg: &CameraFitConfig,
) -> Result<Vec<[f32; 3]>, Error> {
    let w = developed.image.width() as usize;
    let h = developed.image.height() as usize;

    let raw_u16 = developed.image.as_raw();
    let pixels_f32: Vec<f32> = raw_u16.iter().map(|&v| v as f32 / 65535.0).collect();

    let mut patches = Vec::with_capacity(24);
    let mut patch_idx = 0;

    for &cy in &cam_cfg.row_centres_y {
        for &cx in &cam_cfg.column_centres_x {
            let patch = measure_patch_box(
                &pixels_f32,
                w,
                h,
                cx,
                cy,
                cam_cfg.box_size,
                &cam_cfg.raw,
                patch_idx,
                true,
            )?;
            patches.push(patch);
            patch_idx += 1;
        }
    }

    Ok(patches)
}

/// Measures all 24 patches from camera JPEG image in sRGB [0, 1], and detects clipping.
pub fn measure_jpeg_patches(
    jpeg_img: &RgbImage,
    jpeg_name: &str,
    cam_cfg: &CameraFitConfig,
) -> Result<(Vec<[f32; 3]>, Vec<bool>), Error> {
    let w = jpeg_img.width() as usize;
    let h = jpeg_img.height() as usize;

    let raw_u8 = jpeg_img.as_raw();
    let pixels_f32: Vec<f32> = raw_u8.iter().map(|&v| v as f32 / 255.0).collect();

    let mut patches = Vec::with_capacity(24);
    let mut clipped = Vec::with_capacity(24);
    let mut patch_idx = 0;

    for &cy in &cam_cfg.row_centres_y {
        for &cx in &cam_cfg.column_centres_x {
            let patch = measure_patch_box(
                &pixels_f32,
                w,
                h,
                cx,
                cy,
                cam_cfg.box_size,
                jpeg_name,
                patch_idx,
                true,
            )?;
            let is_clipped = patch.iter().any(|&v| {
                let u8_val = v * 255.0;
                u8_val <= 1.0 || u8_val >= 254.0
            });

            patches.push(patch);
            clipped.push(is_clipped);
            patch_idx += 1;
        }
    }

    Ok((patches, clipped))
}

/// Computes exposure gain g over the 4 middle greys (patches 1, 2, 3, 4).
pub fn compute_exposure_gain(
    ours_patches_pro: &[[f32; 3]],
    target_patches_srgb: &[[f32; 3]],
) -> f32 {
    let mut ratios = Vec::with_capacity(4);

    let m_srgb_to_d50 = looks_colour::srgb_linear_to_xyz_d50_matrix();
    let m_pro_to_d50 = dev_colour::prophoto_to_xyz_d50();

    for idx in 1..=4 {
        let p_ours = ours_patches_pro[idx];
        let lin_ours = [
            tone::prophoto_decode(p_ours[0]),
            tone::prophoto_decode(p_ours[1]),
            tone::prophoto_decode(p_ours[2]),
        ];
        let xyz_ours = dev_colour::mul_3x3_vec(m_pro_to_d50, lin_ours);
        let y_ours = xyz_ours[1];

        let p_target = target_patches_srgb[idx];
        let lin_target = [
            crate::media::develop::render::srgb_to_linear(p_target[0]),
            crate::media::develop::render::srgb_to_linear(p_target[1]),
            crate::media::develop::render::srgb_to_linear(p_target[2]),
        ];
        let xyz_target = dev_colour::mul_3x3_vec(m_srgb_to_d50, lin_target);
        let y_target = xyz_target[1];

        if y_ours > 1e-6 {
            ratios.push(y_target / y_ours);
        }
    }

    ratios.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    if ratios.len() == 4 {
        0.5 * (ratios[1] + ratios[2])
    } else if !ratios.is_empty() {
        ratios[ratios.len() / 2]
    } else {
        1.0
    }
}

/// Solves linear system A * x = b using Gaussian elimination with partial pivoting.
#[allow(clippy::needless_range_loop)]
fn solve_linear(a: &[Vec<f32>], b: &[f32]) -> Option<Vec<f32>> {
    let n = b.len();
    let mut mat = vec![vec![0.0f32; n + 1]; n];
    for i in 0..n {
        for j in 0..n {
            mat[i][j] = a[i][j];
        }
        mat[i][n] = b[i];
    }

    for col in 0..n {
        let mut max_row = col;
        let mut max_val = mat[col][col].abs();
        for row in (col + 1)..n {
            let val = mat[row][col].abs();
            if val > max_val {
                max_val = val;
                max_row = row;
            }
        }
        if max_val < 1e-12 {
            return None;
        }
        if max_row != col {
            mat.swap(col, max_row);
        }

        let pivot = mat[col][col];
        for row in (col + 1)..n {
            let factor = mat[row][col] / pivot;
            for j in col..=n {
                mat[row][j] -= factor * mat[col][j];
            }
        }
    }

    let mut x = vec![0.0f32; n];
    for i in (0..n).rev() {
        let mut sum = mat[i][n];
        for j in (i + 1)..n {
            sum -= mat[i][j] * x[j];
        }
        x[i] = sum / mat[i][i];
    }

    Some(x)
}

/// Levenberg-Marquardt optimizer for LookModel parameters.
#[allow(clippy::needless_range_loop)]
pub fn fit_look_model(
    ours_patches_pro: &[[f32; 3]],
    target_labs_d50: &[[f32; 3]],
    clipped: &[bool],
    g: f32,
    hold_out_idx: Option<usize>,
) -> Result<LookModel, Error> {
    let k = KNOTS_K;
    let num_params = 9 + 3 * k;

    // Initial parameter vector: M = I (9), theta = 0 (3K)
    let mut p = vec![0.0f32; num_params];
    p[0] = 1.0;
    p[4] = 1.0;
    p[8] = 1.0;

    let weights: Vec<f32> = (0..24)
        .map(|i| {
            if Some(i) == hold_out_idx {
                0.0
            } else if clipped[i] {
                0.25
            } else if i < 6 {
                2.0
            } else {
                1.0
            }
        })
        .collect();

    let calc_residuals = |params: &[f32]| -> (Vec<f32>, f32) {
        let model = LookModel::from_params(g, params, k);
        let mut res = Vec::with_capacity(24 * 3 + 9);

        // Patch residuals: w * (Lab(model(ours)) - Lab(camera))
        for (i, patch_ours) in ours_patches_pro.iter().enumerate() {
            let w = weights[i];
            if w == 0.0 {
                res.push(0.0);
                res.push(0.0);
                res.push(0.0);
                continue;
            }

            let model_out = model.eval_direct(*patch_ours);
            let lin_pro = [
                tone::prophoto_decode(model_out[0]),
                tone::prophoto_decode(model_out[1]),
                tone::prophoto_decode(model_out[2]),
            ];
            let model_lab = looks_colour::prophoto_linear_to_lab_d50(lin_pro);
            let target_lab = target_labs_d50[i];

            res.push(w * (model_lab[0] - target_lab[0]));
            res.push(w * (model_lab[1] - target_lab[1]));
            res.push(w * (model_lab[2] - target_lab[2]));
        }

        // Ridge penalty: 0.02 * (M - I)
        let eye = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        for j in 0..9 {
            res.push(0.02 * (params[j] - eye[j]));
        }

        let cost: f32 = 0.5 * res.iter().map(|&r| r * r).sum::<f32>();
        (res, cost)
    };

    let (mut res, mut cost) = calc_residuals(&p);
    let mut lambda = 1e-3f32;
    let max_iter = 120;
    let h_step = 1e-6f32;

    for _iter in 0..max_iter {
        let num_res = res.len();

        // Numerical Jacobian J (num_res x num_params)
        let mut j_mat = vec![vec![0.0f32; num_params]; num_res];
        for j in 0..num_params {
            let mut p_step = p.clone();
            p_step[j] += h_step;
            let (res_step, _) = calc_residuals(&p_step);
            for i in 0..num_res {
                j_mat[i][j] = (res_step[i] - res[i]) / h_step;
            }
        }

        // A = J^T J, g_vec = J^T res
        let mut a_mat = vec![vec![0.0f32; num_params]; num_params];
        let mut g_vec = vec![0.0f32; num_params];

        for i in 0..num_res {
            let r_i = res[i];
            for j in 0..num_params {
                g_vec[j] += j_mat[i][j] * r_i;
                for l in 0..=j {
                    a_mat[j][l] += j_mat[i][j] * j_mat[i][l];
                }
            }
        }
        for j in 0..num_params {
            for l in (j + 1)..num_params {
                a_mat[j][l] = a_mat[l][j];
            }
        }

        let mut solved = false;
        for _ in 0..10 {
            let mut a_damped = a_mat.clone();
            for j in 0..num_params {
                let diag = a_mat[j][j].max(1e-3);
                a_damped[j][j] += lambda * diag;
            }

            let rhs: Vec<f32> = g_vec.iter().map(|&v| -v).collect();
            if let Some(delta) = solve_linear(&a_damped, &rhs) {
                let mut p_new = p.clone();
                for j in 0..num_params {
                    p_new[j] += delta[j];
                }

                let (res_new, cost_new) = calc_residuals(&p_new);
                if cost_new < cost {
                    let model_candidate = LookModel::from_params(g, &p_new, k);
                    if model_candidate.det_m() > 0.0 {
                        p = p_new;
                        res = res_new;
                        cost = cost_new;
                        lambda = (lambda * 0.1).max(1e-7);
                        solved = true;
                        break;
                    }
                }
            }
            lambda *= 10.0;
        }

        if !solved {
            break;
        }
    }

    let final_model = LookModel::from_params(g, &p, k);
    let det = final_model.det_m();
    if det <= 0.0 {
        return Err(Error::Refused(format!(
            "fitted matrix determinant is non-positive: {det:.6}"
        )));
    }

    Ok(final_model)
}

/// Bakes model to 33x33x33 Adobe .cube format.
pub fn bake_cube_33(model: &LookModel, title: &str) -> String {
    use std::fmt::Write;
    let size = 33;
    let mut out = String::with_capacity(33 * 33 * 33 * 32);
    let _ = writeln!(&mut out, "TITLE \"{title}\"");
    let _ = writeln!(&mut out, "LUT_3D_SIZE {size}");

    let step = 1.0 / (size - 1) as f32;
    for b in 0..size {
        let in_b = b as f32 * step;
        for g in 0..size {
            let in_g = g as f32 * step;
            for r in 0..size {
                let in_r = r as f32 * step;
                let [out_r, out_g, out_b] = model.eval([in_r, in_g, in_b]);
                let _ = writeln!(
                    &mut out,
                    "{:.7} {:.7} {:.7}",
                    out_r.clamp(0.0, 1.0),
                    out_g.clamp(0.0, 1.0),
                    out_b.clamp(0.0, 1.0)
                );
            }
        }
    }
    out
}

/// Leave-one-out cross validation metrics over the 18 colour patches (indices 6..24).
#[derive(Debug, Clone, Copy)]
pub struct LeaveOneOutResult {
    pub mean_delta_e: f32,
    pub max_delta_e: f32,
}

pub fn evaluate_leave_one_out(
    ours_patches_pro: &[[f32; 3]],
    target_labs_d50: &[[f32; 3]],
    clipped: &[bool],
    g: f32,
) -> Result<LeaveOneOutResult, Error> {
    let mut delta_es = Vec::with_capacity(18);

    for hold_out in 6..24 {
        let model = fit_look_model(
            ours_patches_pro,
            target_labs_d50,
            clipped,
            g,
            Some(hold_out),
        )?;
        let held_out_ours = ours_patches_pro[hold_out];
        let model_out = model.eval(held_out_ours);
        let lin_pro = [
            tone::prophoto_decode(model_out[0]),
            tone::prophoto_decode(model_out[1]),
            tone::prophoto_decode(model_out[2]),
        ];
        let model_lab = looks_colour::prophoto_linear_to_lab_d50(lin_pro);
        let target_lab = target_labs_d50[hold_out];
        let de = looks_colour::ciede2000(model_lab, target_lab);
        delta_es.push(de);
    }

    let mean_de = delta_es.iter().sum::<f32>() / delta_es.len() as f32;
    let max_de = delta_es.iter().cloned().fold(0.0f32, |acc, v| acc.max(v));

    Ok(LeaveOneOutResult {
        mean_delta_e: mean_de,
        max_delta_e: max_de,
    })
}

/// Evaluates quality through the shipped path: applies baked .cube to developed chart as TIFF,
/// decodes TIFF, measures patches, and scores Delta E 2000.
#[derive(Debug, Clone)]
pub struct ShippedPathEvaluation {
    pub per_patch_delta_e: Vec<f32>,
    pub mean_delta_e: f32,
    pub max_unclipped_delta_e: f32,
    pub passed: bool,
    pub failure_reasons: Vec<String>,
}

pub fn evaluate_shipped_path(
    developed_chart: &Developed,
    cube_bytes: &[u8],
    cam_cfg: &CameraFitConfig,
    target_labs_d50: &[[f32; 3]],
    clipped: &[bool],
    loo_mean: f32,
) -> Result<ShippedPathEvaluation, Error> {
    let lut = Lut::from_bytes("test.cube", cube_bytes)?;
    let look = Look::prophoto(lut);

    let rendered = render(developed_chart, Some(&look), OutputFormat::Tiff)?;
    let tiff_dyn = image::load_from_memory_with_format(&rendered.bytes, image::ImageFormat::Tiff)
        .map_err(|e| Error::Internal(format!("Failed to decode rendered TIFF: {e}")))?;
    let tiff_rgb16 = tiff_dyn.into_rgb16();

    let w = tiff_rgb16.width() as usize;
    let h = tiff_rgb16.height() as usize;
    let raw_u16 = tiff_rgb16.as_raw();
    let pixels_f32: Vec<f32> = raw_u16.iter().map(|&v| v as f32 / 65535.0).collect();

    let mut per_patch_delta_e = Vec::with_capacity(24);
    let mut patch_idx = 0;

    for &cy in &cam_cfg.row_centres_y {
        for &cx in &cam_cfg.column_centres_x {
            let measured = measure_patch_box(
                &pixels_f32,
                w,
                h,
                cx,
                cy,
                cam_cfg.box_size,
                "shipped_tiff",
                patch_idx,
                false,
            )?;
            let lin_pro = [
                tone::prophoto_decode(measured[0]),
                tone::prophoto_decode(measured[1]),
                tone::prophoto_decode(measured[2]),
            ];
            let lab_d50 = looks_colour::prophoto_linear_to_lab_d50(lin_pro);
            let target_lab = target_labs_d50[patch_idx];
            let de = looks_colour::ciede2000(lab_d50, target_lab);
            per_patch_delta_e.push(de);
            patch_idx += 1;
        }
    }

    let mean_delta_e = per_patch_delta_e.iter().sum::<f32>() / 24.0;
    let mut max_unclipped = 0.0f32;
    for (i, &de) in per_patch_delta_e.iter().enumerate() {
        if !clipped[i] && de > max_unclipped {
            max_unclipped = de;
        }
    }

    let mut failure_reasons = Vec::new();
    if mean_delta_e > THRESHOLD_MEAN_DELTA_E {
        failure_reasons.push(format!(
            "mean ΔE2000 {mean_delta_e:.2} exceeds threshold {THRESHOLD_MEAN_DELTA_E:.1}"
        ));
    }
    if max_unclipped > THRESHOLD_MAX_DELTA_E {
        failure_reasons.push(format!(
            "max unclipped ΔE2000 {max_unclipped:.2} exceeds threshold {THRESHOLD_MAX_DELTA_E:.1}"
        ));
    }
    if loo_mean > THRESHOLD_LOO_MEAN_DELTA_E {
        failure_reasons.push(format!(
            "leave-one-out mean ΔE2000 {loo_mean:.2} exceeds threshold {THRESHOLD_LOO_MEAN_DELTA_E:.1}"
        ));
    }

    let passed = failure_reasons.is_empty();

    Ok(ShippedPathEvaluation {
        per_patch_delta_e,
        mean_delta_e,
        max_unclipped_delta_e: max_unclipped,
        passed,
        failure_reasons,
    })
}
