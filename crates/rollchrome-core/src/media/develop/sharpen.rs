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

/// Output sharpening strength for the neutral picture (DV-14).
///
/// Mild band-limited sharpening: luminance + 0.3 * (blur σ 1 - blur σ 3), which matches
/// Apple's rendering at 1–4 px frequencies without sharpening the finest scale.
pub const OUTPUT_SHARPEN: f32 = 0.3;

/// Output sharpening for the developed 16-bit ProPhoto neutral image (DV-14).
///
/// Runs on the developed 16-bit ProPhoto image, ROMM-encoded, after the tone curve:
/// 1. Y = (0.288040·R + 0.711874·G + 0.000086·B) / 65535 per pixel.
/// 2. b1 = Gaussian blur of Y with σ 1.0, and b3 with σ 3.0, in full-resolution pixels.
/// 3. d = OUTPUT_SHARPEN × (b1 − b3) × 65535. Add the same d to R, G and B, round, and clamp to 0..=65535.
pub fn output_sharpen(image: &mut image::ImageBuffer<image::Rgb<u16>, Vec<u16>>) {
    let w = image.width() as usize;
    let h = image.height() as usize;
    if w == 0 || h == 0 {
        return;
    }

    let y_plane: Vec<f32> = image
        .par_chunks_exact(3)
        .map(|px| {
            (px[0] as f32 * Y_WEIGHTS[0]
                + px[1] as f32 * Y_WEIGHTS[1]
                + px[2] as f32 * Y_WEIGHTS[2])
                / 65535.0
        })
        .collect();

    let k1 = kernel(1.0);
    let k3 = kernel(3.0);
    let b1 = blur(&y_plane, w, h, &k1);
    let b3 = blur(&y_plane, w, h, &k3);

    image
        .par_chunks_exact_mut(3)
        .zip(b1.par_iter().zip(b3.par_iter()))
        .for_each(|(px, (&b1_val, &b3_val))| {
            let d = OUTPUT_SHARPEN * (b1_val - b3_val) * 65535.0;
            px[0] = (px[0] as f32 + d).round().clamp(0.0, 65535.0) as u16;
            px[1] = (px[1] as f32 + d).round().clamp(0.0, 65535.0) as u16;
            px[2] = (px[2] as f32 + d).round().clamp(0.0, 65535.0) as u16;
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_sharpening_leaves_a_flat_image_unchanged() {
        let w = 64;
        let h = 16;
        let mut img =
            image::ImageBuffer::from_pixel(w, h, image::Rgb([30000u16, 30000u16, 30000u16]));
        let orig = img.clone();
        output_sharpen(&mut img);
        assert_eq!(img, orig);
    }

    #[test]
    fn output_sharpening_lifts_both_sides_of_an_edge_and_keeps_grey() {
        let w = 64;
        let h = 16;
        let mut img = image::ImageBuffer::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let v = if x < 32 { 19661u16 } else { 39321u16 };
                img.put_pixel(x, y, image::Rgb([v, v, v]));
            }
        }

        output_sharpen(&mut img);

        // R = G = B at every pixel
        for p in img.pixels() {
            assert_eq!(p[0], p[1]);
            assert_eq!(p[1], p[2]);
        }

        // Row 8 must read, within ±2:
        // x = 0 → 19661, 28 → 18953, 30 → 18192, 31 → 18877, 32 → 40105, 33 → 40790, 35 → 40029, 63 → 39321
        let expected = [
            (0, 19661i32),
            (28, 18953i32),
            (30, 18192i32),
            (31, 18877i32),
            (32, 40105i32),
            (33, 40790i32),
            (35, 40029i32),
            (63, 39321i32),
        ];

        for (x, exp) in expected {
            let actual = img.get_pixel(x, 8)[0] as i32;
            let diff = (actual - exp).abs();
            assert!(
                diff <= 2,
                "At x={x}, y=8: actual {actual}, expected {exp} (diff {diff} > 2)"
            );
        }
    }
}
