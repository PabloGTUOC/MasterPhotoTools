//! Capture sharpening: restoring what the lens, the anti-aliasing filter and demosaicing blurred (DV-10).
//!
//! Richardson–Lucy deconvolution with a Gaussian blur model, on linear luminance:
//!
//! - **Luminance only.** Each pixel's RGB is scaled by its luminance gain, so hue and saturation keep, and
//!   colour noise and fringes are not sharpened.
//! - **Clamped to the 3×3 range of the original.** Deconvolution rings at a hard edge; clamping each
//!   pixel to what its neighbourhood already spanned removes the halo by construction.
//! - **Masked by local contrast.** Where the picture is flat — sky, skin — the deconvolution would only
//!   sharpen noise the noise reduction just removed, so it fades out there.
//!
//! It runs on the linear image after lens correction and noise reduction and before the look, so film
//! grain, added later, is never sharpened.

use rayon::prelude::*;

/// Richardson–Lucy iterations. Ten recover most of a σ 0.7 blur; more begin to lift noise first.
const ITERATIONS: usize = 10;

/// ProPhoto RGB's luminance weights (the Y row of its matrix to XYZ).
const Y_WEIGHTS: [f32; 3] = [0.288_040, 0.711_874, 0.000_086];

/// The blur radius for a sensor: Bayer demosaics sharper than X-Trans's 6×6 pattern.
pub fn sigma_for(xtrans: bool) -> f32 {
    if xtrans {
        0.85
    } else {
        0.7
    }
}

/// A normalised Gaussian kernel, radius 3σ.
fn kernel(sigma: f32) -> Vec<f32> {
    let r = (3.0 * sigma).ceil().max(1.0) as i32;
    let mut k: Vec<f32> = (-r..=r)
        .map(|i| (-(i * i) as f32 / (2.0 * sigma * sigma)).exp())
        .collect();
    let sum: f32 = k.iter().sum();
    k.iter_mut().for_each(|v| *v /= sum);
    k
}

/// Separable Gaussian blur of one plane, edges clamped.
fn blur(plane: &[f32], w: usize, h: usize, k: &[f32]) -> Vec<f32> {
    let r = (k.len() / 2) as isize;
    let mut tmp = vec![0.0f32; w * h];
    tmp.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        let src = &plane[y * w..(y + 1) * w];
        for (x, out) in row.iter_mut().enumerate() {
            let mut acc = 0.0;
            for (i, kv) in k.iter().enumerate() {
                let sx = (x as isize + i as isize - r).clamp(0, w as isize - 1) as usize;
                acc += src[sx] * kv;
            }
            *out = acc;
        }
    });
    let mut out = vec![0.0f32; w * h];
    out.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (x, o) in row.iter_mut().enumerate() {
            let mut acc = 0.0;
            for (i, kv) in k.iter().enumerate() {
                let sy = (y as isize + i as isize - r).clamp(0, h as isize - 1) as usize;
                acc += tmp[sy * w + x] * kv;
            }
            *o = acc;
        }
    });
    out
}

/// Sharpens linear pixels. `amount` 0–100 blends from the input to the full deconvolution; `sigma` is
/// the blur being undone, in pixels (see [`sigma_for`]).
pub fn capture_sharpen(
    width: u32,
    height: u32,
    pixels: &[[f32; 3]],
    amount: f32,
    sigma: f32,
) -> Vec<[f32; 3]> {
    let strength = (amount / 100.0).clamp(0.0, 1.0);
    if strength == 0.0 || sigma <= 0.0 {
        return pixels.to_vec();
    }
    let (w, h) = (width as usize, height as usize);
    let luma: Vec<f32> = pixels
        .par_iter()
        .map(|p| (p[0] * Y_WEIGHTS[0] + p[1] * Y_WEIGHTS[1] + p[2] * Y_WEIGHTS[2]).max(0.0))
        .collect();
    let k = kernel(sigma);
    const EPS: f32 = 1e-6;

    // Richardson–Lucy: estimate ← estimate · blur(observed / blur(estimate)). The kernel is
    // symmetric, so its mirror is itself.
    let mut estimate = luma.clone();
    for _ in 0..ITERATIONS {
        let blurred = blur(&estimate, w, h, &k);
        let ratio: Vec<f32> = luma
            .par_iter()
            .zip(&blurred)
            .map(|(o, b)| o / b.max(EPS))
            .collect();
        let correction = blur(&ratio, w, h, &k);
        estimate
            .par_iter_mut()
            .zip(&correction)
            .for_each(|(e, c)| *e *= c);
    }

    // Local contrast for the mask: the original against its own blur, relative to its level.
    let soft = blur(&luma, w, h, &k);

    let mut out = vec![[0.0f32; 3]; w * h];
    out.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (x, px) in row.iter_mut().enumerate() {
            let i = y * w + x;
            // The 3×3 range of the original: no overshoot past what was already there.
            let (mut lo, mut hi) = (f32::MAX, f32::MIN);
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    let xx = (x as i32 + dx).clamp(0, w as i32 - 1) as usize;
                    let yy = (y as i32 + dy).clamp(0, h as i32 - 1) as usize;
                    let v = luma[yy * w + xx];
                    lo = lo.min(v);
                    hi = hi.max(v);
                }
            }
            let sharp = estimate[i].clamp(lo, hi);
            // Relative contrast below 0.5 % is flat; above 2.5 % is detail.
            let contrast = (luma[i] - soft[i]).abs() / (soft[i] + 0.01);
            let mask = ((contrast - 0.005) / 0.02).clamp(0.0, 1.0);
            let target = luma[i] + (sharp - luma[i]) * strength * mask;
            let gain = if luma[i] > EPS { target / luma[i] } else { 1.0 };
            let p = pixels[i];
            *px = [p[0] * gain, p[1] * gain, p[2] * gain];
        }
    });
    out
}
