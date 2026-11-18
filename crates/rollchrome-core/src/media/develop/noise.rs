//! Noise measurement and scaling for RAW development (DV-14).
//!
//! Measures noise on the scene-referred linear ProPhoto RGB image using Donoho's robust
//! wavelet-based estimator on 2x2 blocks, and scales the noise reduction slider so that
//! cleaner sensors and lower ISOs are not over-smoothed.

use rayon::prelude::*;

/// Noise level reference (0.005).
///
/// The median of the owner's XZ-1 files at ISO 400, measured after the camera's
/// geometry, the files on which DV-12 tuned 25.
pub const NOISE_REFERENCE: f32 = 0.005;

/// Measures the sensor noise level on scene-referred linear ProPhoto RGB.
///
/// Works on ProPhoto sqrt-luminance `y = sqrt(max(0, 0.288040·R + 0.711874·G + 0.000086·B))`.
/// For every 2x2 block whose top-left is at even (x, y) with x + 1 < width and y + 1 < height:
/// `d = |y(x,y) - y(x+1,y) - y(x,y+1) + y(x+1,y+1)| / 2`.
/// Returns Donoho's robust estimate `median(d) / 0.6745`.
///
/// If width < 2, height < 2 or `pixels.len() != width * height`, returns `NOISE_REFERENCE`.
pub fn noise_level(pixels: &[[f32; 3]], width: u32, height: u32) -> f32 {
    let w = width as usize;
    let h = height as usize;
    if width < 2 || height < 2 || pixels.len() != w * h {
        return NOISE_REFERENCE;
    }

    let y_plane: Vec<f32> = pixels
        .par_iter()
        .map(|p| {
            (0.288_040 * p[0] + 0.711_874 * p[1] + 0.000_086 * p[2])
                .max(0.0)
                .sqrt()
        })
        .collect();

    let y_indices: Vec<usize> = (0..h).step_by(2).filter(|&y| y + 1 < h).collect();
    let y_ref = &y_plane;
    let mut diffs: Vec<f32> = y_indices
        .into_par_iter()
        .flat_map_iter(|y| {
            let x_indices = (0..w).step_by(2).filter(|&x| x + 1 < w);
            x_indices.map(move |x| {
                let y00 = y_ref[y * w + x];
                let y10 = y_ref[y * w + x + 1];
                let y01 = y_ref[(y + 1) * w + x];
                let y11 = y_ref[(y + 1) * w + x + 1];
                ((y00 - y10 - y01 + y11) / 2.0).abs()
            })
        })
        .collect();

    if diffs.is_empty() {
        return NOISE_REFERENCE;
    }

    let mid = diffs.len() / 2;
    diffs.select_nth_unstable_by(mid, |a, b| a.total_cmp(b));
    diffs[mid] / 0.6745
}

/// Returns the effective noise reduction strength scaled by measured noise level.
///
/// Scales `amount` by `level / NOISE_REFERENCE`, clamped to 0..=100.
/// If `level` is not finite or < 0, returns `amount` unchanged.
/// If `amount == 0.0`, returns 0.0.
pub fn effective_noise_reduction(amount: f32, level: f32) -> f32 {
    if amount == 0.0 {
        return 0.0;
    }
    if !level.is_finite() || level < 0.0 {
        return amount;
    }
    (amount * level / NOISE_REFERENCE).clamp(0.0, 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Lcg(u64);
    impl Lcg {
        fn next_u32(&mut self) -> u32 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (self.0 >> 32) as u32
        }
        fn next_f32(&mut self) -> f32 {
            (self.next_u32() >> 8) as f32 / 16777216.0
        }
        fn next_gaussian(&mut self, sigma: f32) -> f32 {
            let u1 = self.next_f32().max(1e-7);
            let u2 = self.next_f32();
            (-2.0 * u1.ln()).sqrt() * (2.0 * std::f32::consts::PI * u2).cos() * sigma
        }
    }

    fn generate_noise_image(w: u32, h: u32, s: f32, seed: u64) -> Vec<[f32; 3]> {
        let mut rng = Lcg(seed);
        let mut pixels = Vec::with_capacity((w * h) as usize);
        for _ in 0..(w * h) {
            let n = rng.next_gaussian(s);
            let v = (0.5 + n).powi(2);
            pixels.push([v, v, v]);
        }
        pixels
    }

    fn sqrt_luma_std(pixels: &[[f32; 3]]) -> f32 {
        let n = pixels.len() as f64;
        let lumas: Vec<f64> = pixels
            .iter()
            .map(|p| {
                (0.288_040 * p[0] + 0.711_874 * p[1] + 0.000_086 * p[2])
                    .max(0.0)
                    .sqrt() as f64
            })
            .collect();
        let mean = lumas.iter().sum::<f64>() / n;
        let var = lumas.iter().map(|&y| (y - mean).powi(2)).sum::<f64>() / n;
        var.sqrt() as f32
    }

    #[test]
    fn noise_level_recovers_the_noise_put_in() {
        for s in [0.0025, 0.01] {
            let pixels = generate_noise_image(400, 300, s, 42);
            let measured = noise_level(&pixels, 400, 300);
            let diff = (measured - s).abs();
            let relative_error = diff / s;
            assert!(
                relative_error <= 0.03,
                "For s={s}, measured {measured} (relative error {relative_error:.4} > 3%)"
            );
        }
    }

    #[test]
    fn a_flat_or_tiny_image_has_no_noise_or_no_scaling() {
        let flat = vec![[0.18, 0.18, 0.18]; 100 * 100];
        assert_eq!(noise_level(&flat, 100, 100), 0.0);

        let one_x_n = vec![[0.18, 0.18, 0.18]; 10];
        assert_eq!(noise_level(&one_x_n, 1, 10), NOISE_REFERENCE);

        let n_x_one = vec![[0.18, 0.18, 0.18]; 10];
        assert_eq!(noise_level(&n_x_one, 10, 1), NOISE_REFERENCE);

        let wrong_len = vec![[0.18, 0.18, 0.18]; 99];
        assert_eq!(noise_level(&wrong_len, 10, 10), NOISE_REFERENCE);
    }

    #[test]
    fn the_effective_amount_follows_the_noise() {
        let cases = [
            (25.0, 0.005, 25.0),
            (25.0, 0.0025, 12.5),
            (25.0, 0.01, 50.0),
            (25.0, 0.05, 100.0),
            (0.0, 0.01, 0.0),
            (25.0, f32::NAN, 25.0),
            (25.0, -1.0, 25.0),
        ];

        for (amount, level, expected) in cases {
            let eff = effective_noise_reduction(amount, level);
            assert!(
                (eff - expected).abs() < 1e-4,
                "effective_noise_reduction({amount}, {level}) = {eff}, expected {expected}"
            );
        }
    }

    #[test]
    fn noise_reduction_scaled_to_the_noise_keeps_the_same_share_of_it() {
        let sigmas = [0.0025, 0.005, 0.01];
        let mut scaled_shares = Vec::new();
        let mut fixed_shares = Vec::new();

        for (i, &s) in sigmas.iter().enumerate() {
            let pixels = generate_noise_image(400, 300, s, 1000 + i as u64);
            let std_before = sqrt_luma_std(&pixels);

            let level = noise_level(&pixels, 400, 300);
            let eff_nr = effective_noise_reduction(25.0, level);
            let denoised_scaled =
                crate::media::develop::reduce_noise(400, 300, &pixels, eff_nr, 1.0);
            let std_after_scaled = sqrt_luma_std(&denoised_scaled);
            let share_scaled = std_after_scaled / std_before;
            scaled_shares.push(share_scaled);

            let denoised_fixed = crate::media::develop::reduce_noise(400, 300, &pixels, 25.0, 1.0);
            let std_after_fixed = sqrt_luma_std(&denoised_fixed);
            let share_fixed = std_after_fixed / std_before;
            fixed_shares.push(share_fixed);
        }

        for (s, &share) in sigmas.iter().zip(scaled_shares.iter()) {
            assert!(
                (0.15..=0.27).contains(&share),
                "For s={s}, scaled share {share} not in 0.15..=0.27"
            );
        }

        for i in 0..scaled_shares.len() {
            for j in (i + 1)..scaled_shares.len() {
                let diff = (scaled_shares[i] - scaled_shares[j]).abs();
                assert!(
                    diff <= 0.02,
                    "Scaled shares {} and {} differ by {diff} > 0.02",
                    scaled_shares[i],
                    scaled_shares[j]
                );
            }
        }

        let fixed_diff = (fixed_shares[0] - fixed_shares[2]).abs();
        assert!(
            fixed_diff > 0.2,
            "Fixed shares {} (s=0.0025) and {} (s=0.01) differ by {fixed_diff} <= 0.2",
            fixed_shares[0],
            fixed_shares[2]
        );
    }
}
