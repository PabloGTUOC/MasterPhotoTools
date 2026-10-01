//! Scale-independent deterministic film grain in the display domain (ED-14).
//!
//! ### Display Domain & Luminance-Only Model
//! Real photographic film grain arises from silver halide crystals (black and white film)
//! or dye clouds (color negative/transparency film). Because human visual contrast sensitivity
//! follows Weber's law across display luminances, grain is evaluated in the **display-encoded domain**
//! (after non-linear sRGB transfer, tone curves, colour grading, and 3D LUT application). This
//! guarantees uniform visual texture density across tones without shadow crushing or highlight clipping.
//!
//! Grain perturbation is **luminance-only**: the identical scalar delta $\delta$ is applied to
//! $R$, $G$, and $B$, preserving chromaticity ratios without introducing unsightly color noise.
//! Furthermore, the amplitude is envelope-scaled by a parabolic fade factor $4 Y (1 - Y)$
//! (where $Y$ is display luminance $\in [0.0, 1.0]$), which smoothly vanishes towards pure
//! black ($Y \to 0$) and pure white ($Y \to 1$), ensuring that neither deep shadow detail nor
//! specular highlights are pushed into clipping.
//!
//! ### Scale Independence & Determinism
//! Grain size is parameterized as a fraction of the image width $W$ using normalized coordinates
//! $u = (x + 0.5)/W$ and $v = (y + 0.5)/W$. A 720p dragging proxy and a 36 MP export sample the
//! identical continuous noise field, producing visual scale independence when comparing downscaled
//! outputs. The generator uses SplitMix64 hash PRNG; evaluating any pixel $(x, y)$ is a pure
//! stateless function of `(seed, u, v)`, guaranteeing byte-identical output across 1 to $N$ threads.

use serde::{Deserialize, Serialize};

fn default_twenty_five() -> f32 {
    25.0
}

fn default_fifty() -> f32 {
    50.0
}

/// Scale-independent film grain parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilmGrain {
    /// Grain intensity from 0.0 (disabled / identity) to 100.0 (maximum grain). Default: 0.0.
    #[serde(default)]
    pub amount: f32,
    /// Spatial grain clumping scale from 0.0 (fine 35mm grain) to 100.0 (coarse pushed grain). Default: 25.0.
    #[serde(default = "default_twenty_five")]
    pub size: f32,
    /// Texture roughness / high-frequency fractal content from 0.0 (smooth) to 100.0 (sharp). Default: 50.0.
    #[serde(default = "default_fifty")]
    pub roughness: f32,
}

impl Default for FilmGrain {
    fn default() -> Self {
        Self {
            amount: 0.0,
            size: 25.0,
            roughness: 50.0,
        }
    }
}

impl FilmGrain {
    /// Returns true if the film grain is at rest (amount == 0.0).
    pub fn is_identity(&self) -> bool {
        self.amount.abs() < 1e-4
    }

    /// Precomputes compiled grain parameters for rapid per-pixel evaluation.
    pub fn compile(&self, source_sha256: &str) -> CompiledGrain {
        if self.is_identity() {
            return CompiledGrain::identity();
        }

        let seed = seed_from_source_sha256(source_sha256);
        let s = self.size.clamp(0.0, 100.0);
        // Base frequency: higher frequency (more cells) for small size, lower frequency for large size
        let k_base = 200.0 + (100.0 - s) * 6.0;

        let r = (self.roughness / 100.0).clamp(0.0, 1.0);
        let max_amp = (self.amount / 100.0).clamp(0.0, 1.0) * 0.20;

        CompiledGrain {
            seed,
            k_base,
            roughness_w: r,
            max_amp,
            is_identity: false,
        }
    }
}

/// Converts a photograph's source SHA-256 string into a deterministic 64-bit integer seed.
pub fn seed_from_source_sha256(source_sha256: &str) -> u64 {
    if source_sha256.len() >= 16 {
        if let Ok(val) = u64::from_str_radix(&source_sha256[..16], 16) {
            return val;
        }
    }
    // FNV-1a hash fallback for non-hex or short strings
    let mut h = 0xcbf29ce484222325u64;
    for b in source_sha256.as_bytes() {
        h = (h ^ (*b as u64)).wrapping_mul(0x100000001b3);
    }
    h
}

/// Precomputed grain generator parameters.
#[derive(Debug, Clone, Copy)]
pub struct CompiledGrain {
    pub seed: u64,
    pub k_base: f32,
    pub roughness_w: f32,
    pub max_amp: f32,
    pub is_identity: bool,
}

impl CompiledGrain {
    pub fn identity() -> Self {
        Self {
            seed: 0,
            k_base: 1.0,
            roughness_w: 0.0,
            max_amp: 0.0,
            is_identity: true,
        }
    }

    /// Evaluates the grain luminance delta for a pixel at coordinates `(x, y)` in an image of width `width`.
    #[inline(always)]
    pub fn sample_delta(
        &self,
        x: f32,
        y: f32,
        width: f32,
        disp_r: f32,
        disp_g: f32,
        disp_b: f32,
    ) -> f32 {
        if self.is_identity {
            return 0.0;
        }

        let inv_w = 1.0 / width.max(1.0);
        let u = (x + 0.5) * inv_w;
        let py1 = (y + 0.5) * inv_w * self.k_base;

        self.sample_delta_with_v(u, py1, disp_r, disp_g, disp_b)
    }

    /// Evaluates the grain luminance delta using normalized isotropic coordinate `u = (x + 0.5)/width`
    /// and precomputed vertical frequency coordinate `py1 = (y + 0.5)/width * k_base`.
    #[inline(always)]
    pub fn sample_delta_with_v(
        &self,
        u: f32,
        py1: f32,
        disp_r: f32,
        disp_g: f32,
        disp_b: f32,
    ) -> f32 {
        let rp = self.row_params(py1);
        self.sample_delta_with_row(u, &rp, disp_r, disp_g, disp_b)
    }

    /// Precomputes vertical row interpolation parameters for a given vertical coordinate `py1`.
    #[inline(always)]
    pub fn row_params(&self, py1: f32) -> RowGrainParams {
        let iy1 = py1.floor() as i32;
        let fy1 = py1 - iy1 as f32;
        let sy1 = fy1 * fy1 * (3.0 - 2.0 * fy1);

        let py2 = py1 * 2.0;
        let iy2 = py2.floor() as i32;
        let fy2 = py2 - iy2 as f32;
        let sy2 = fy2 * fy2 * (3.0 - 2.0 * fy2);

        const K_Y: u64 = 0x94d049bb133111eb;
        let y1_term0 = (iy1 as u64).wrapping_mul(K_Y);
        let y1_term1 = ((iy1 + 1) as u64).wrapping_mul(K_Y);
        let y2_term0 = (iy2 as u64).wrapping_mul(K_Y);
        let y2_term1 = ((iy2 + 1) as u64).wrapping_mul(K_Y);

        RowGrainParams {
            y1_term0,
            y1_term1,
            sy1,
            y2_term0,
            y2_term1,
            sy2,
        }
    }

    /// Evaluates the grain luminance delta using row-hoisted vertical interpolation parameters.
    #[inline(always)]
    pub fn sample_delta_with_row(
        &self,
        u: f32,
        rp: &RowGrainParams,
        disp_r: f32,
        disp_g: f32,
        disp_b: f32,
    ) -> f32 {
        if self.is_identity {
            return 0.0;
        }

        // Display luminance Y
        let y_luma = 0.2126 * disp_r + 0.7152 * disp_g + 0.0722 * disp_b;
        if y_luma <= 1e-5 || y_luma >= 1.0 - 1e-5 {
            return 0.0;
        }

        // Parabolic envelope vanishing at 0.0 and 1.0 (peaks at 1.0 for mid-grey 0.5)
        let envelope = 4.0 * y_luma * (1.0 - y_luma);

        // Octave 1: base frequency
        let px1 = u * self.k_base;
        let n1 = sample_noise_2d_row(self.seed, px1, rp.y1_term0, rp.y1_term1, rp.sy1);

        // Octave 2: double frequency for roughness detail
        let n2 = if self.roughness_w > 0.001 {
            let px2 = px1 * 2.0;
            sample_noise_2d_row(
                self.seed.wrapping_add(0x9e3779b97f4a7c15),
                px2,
                rp.y2_term0,
                rp.y2_term1,
                rp.sy2,
            )
        } else {
            0.0
        };

        let noise = (1.0 - 0.4 * self.roughness_w) * n1 + (0.4 * self.roughness_w) * n2;
        noise * self.max_amp * envelope
    }
}

/// Precomputed vertical row interpolation parameters.
#[derive(Debug, Clone, Copy)]
pub struct RowGrainParams {
    pub y1_term0: u64,
    pub y1_term1: u64,
    pub sy1: f32,
    pub y2_term0: u64,
    pub y2_term1: u64,
    pub sy2: f32,
}

impl Default for RowGrainParams {
    fn default() -> Self {
        Self {
            y1_term0: 0,
            y1_term1: 0,
            sy1: 0.0,
            y2_term0: 0,
            y2_term1: 0,
            sy2: 0.0,
        }
    }
}

/// 2D procedural value noise with cubic Hermite interpolation.
#[allow(dead_code)]
#[inline(always)]
fn sample_noise_2d(seed: u64, px: f32, py: f32) -> f32 {
    let iy = py.floor() as i32;
    let fy = py - iy as f32;
    let sy = fy * fy * (3.0 - 2.0 * fy);
    const K_Y: u64 = 0x94d049bb133111eb;
    let y_term0 = (iy as u64).wrapping_mul(K_Y);
    let y_term1 = ((iy + 1) as u64).wrapping_mul(K_Y);
    sample_noise_2d_row(seed, px, y_term0, y_term1, sy)
}

/// 2D procedural value noise with row-hoisted vertical parameters.
#[inline(always)]
fn sample_noise_2d_row(seed: u64, px: f32, y_term0: u64, y_term1: u64, sy: f32) -> f32 {
    let ix = px.floor() as i32;
    let fx = px - ix as f32;

    // Cubic Hermite smoothstep
    let sx = fx * fx * (3.0 - 2.0 * fx);

    let h00 = hash2_y(seed, ix, y_term0);
    let h10 = hash2_y(seed, ix + 1, y_term0);
    let h01 = hash2_y(seed, ix, y_term1);
    let h11 = hash2_y(seed, ix + 1, y_term1);

    let v0 = h00 + sx * (h10 - h00);
    let v1 = h01 + sx * (h11 - h01);
    v0 + sy * (v1 - v0)
}

#[inline(always)]
fn hash2_y(seed: u64, x: i32, y_term: u64) -> f32 {
    let mut z = seed.wrapping_add((x as u64).wrapping_mul(0x9e3779b97f4a7c15));
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    z = z.wrapping_add(y_term);
    z = (z ^ (z >> 27)).wrapping_mul(0x517cc1b727220a95);
    z = z ^ (z >> 31);
    let bits = (z & 0x007fffff) as u32;
    (bits as f32) * (2.0 / 8388607.0) - 1.0
}

/// Stateless SplitMix64 hash mapping integer grid coordinates `(x, y)` to `[-1.0, 1.0]`.
#[allow(dead_code)]
#[inline(always)]
fn hash2(seed: u64, x: i32, y: i32) -> f32 {
    let mut z = seed.wrapping_add((x as u64).wrapping_mul(0x9e3779b97f4a7c15));
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    z = z.wrapping_add((y as u64).wrapping_mul(0x94d049bb133111eb));
    z = (z ^ (z >> 27)).wrapping_mul(0x517cc1b727220a95);
    z = z ^ (z >> 31);
    let bits = (z & 0x007fffff) as u32;
    (bits as f32) * (2.0 / 8388607.0) - 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grain_default_is_identity() {
        let g = FilmGrain::default();
        assert!(g.is_identity());
        let compiled = g.compile("some_sha256");
        let delta = compiled.sample_delta(10.0, 20.0, 100.0, 0.5, 0.5, 0.5);
        assert_eq!(delta, 0.0);
    }

    #[test]
    fn grain_fades_at_pure_black_and_pure_white() {
        let g = FilmGrain {
            amount: 100.0,
            size: 25.0,
            roughness: 50.0,
        };
        let compiled = g.compile("some_sha256");
        // Pure black
        let delta_black = compiled.sample_delta(50.0, 50.0, 100.0, 0.0, 0.0, 0.0);
        assert_eq!(delta_black, 0.0);

        // Pure white
        let delta_white = compiled.sample_delta(50.0, 50.0, 100.0, 1.0, 1.0, 1.0);
        assert_eq!(delta_white, 0.0);

        // Mid-tone has non-zero grain
        let delta_mid = compiled.sample_delta(50.0, 50.0, 100.0, 0.5, 0.5, 0.5);
        assert!(delta_mid.abs() > 1e-4);
    }

    #[test]
    fn seed_from_source_sha256_is_deterministic() {
        let sha = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let seed1 = seed_from_source_sha256(sha);
        let seed2 = seed_from_source_sha256(sha);
        assert_eq!(seed1, seed2);
        assert_ne!(seed1, 0);
    }
}
