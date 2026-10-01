//! Photographic looks (Glow/Bloom, Halation) and optional filmic tone mapper (ED-16).
//!
//! Citations:
//! - Marius Bjørge (SIGGRAPH 2015), "Bandwidth-Efficient Graphics", dual-filter separable
//!   downsampling/upsampling blur pyramid.
//! - Krzysztof Narkowicz (2015), "ACES Filmic Tone Mapping Curve" (Public Domain / CC0).
//!   Closed-form rational curve: f(x) = (x(2.51x + 0.03)) / (x(2.43x + 0.59) + 0.14).
//!
//! # Architecture & Order
//! Both Glow and Halation operate in **scene-linear light** on the output of the geometry
//! stage (post-crop), before the optional tone mapper and display sRGB conversion:
//! 1. Threshold on linear luminance with a soft knee transition.
//! 2. Downsampled to reduced resolution (1/4 linear size) for interactive performance.
//! 3. Evaluated across a multi-scale dual-filter blur pyramid.
//! 4. Upsampled bilinearly and added back to the linear frame.
//!
//! # Halation Tint
//! Film halation occurs when bright light penetrates the emulsion and reflects off the film
//! base back into the red-sensitive bottom emulsion layer. The halation tint is evaluated from
//! OkLCh(L=0.70, C=0.20, h=38.0°) representing photographic red-orange reflection, with normalized
//! linear sRGB vector [1.0000, 0.1292, 0.0304].
//!
//! # Narkowicz ACES Tone Mapper
//! Standard unscaled fit: x = 0.18 maps to ~0.2669, shifting mid-tones upward by ~0.087 for a
//! classic filmic tonal roll-off without highlight clipping. When disabled (default), standard hard
//! clipping at 1.0 preserves exact mathematical identity.

use super::pipeline::LinearBuffer;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

fn default_seventy() -> f32 {
    70.0
}

fn default_thirty() -> f32 {
    30.0
}

fn default_eighty() -> f32 {
    80.0
}

fn default_twenty() -> f32 {
    20.0
}

/// Normalized linear sRGB tint vector for warm red-orange halation (OkLCh h=38.0°).
pub const HALATION_TINT: [f32; 3] = [1.0000, 0.1292, 0.0304];

/// Photographic looks (Glow/Bloom, Halation) and optional filmic tone mapper settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LookEffects {
    /// Glow strength percentage: 0.0 to 100.0 (default: 0.0).
    #[serde(default)]
    pub glow_amount: f32,
    /// Glow luminance threshold percentage: 0.0 to 100.0 (default: 70.0).
    #[serde(default = "default_seventy")]
    pub glow_threshold: f32,
    /// Glow spread radius percentage of image width: 0.0 to 100.0 (default: 30.0).
    #[serde(default = "default_thirty")]
    pub glow_radius: f32,
    /// Halation strength percentage: 0.0 to 100.0 (default: 0.0).
    #[serde(default)]
    pub halation_amount: f32,
    /// Halation luminance threshold percentage: 0.0 to 100.0 (default: 80.0).
    #[serde(default = "default_eighty")]
    pub halation_threshold: f32,
    /// Halation spread radius percentage of image width: 0.0 to 100.0 (default: 20.0).
    #[serde(default = "default_twenty")]
    pub halation_radius: f32,
    /// Optional tone mapper: None (default: hard clip) or Some("aces").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone_mapper: Option<String>,
}

impl Default for LookEffects {
    fn default() -> Self {
        Self {
            glow_amount: 0.0,
            glow_threshold: 70.0,
            glow_radius: 30.0,
            halation_amount: 0.0,
            halation_threshold: 80.0,
            halation_radius: 20.0,
            tone_mapper: None,
        }
    }
}

impl LookEffects {
    /// Returns true if all look controls are at default rest (amounts == 0, tone mapper off).
    pub fn is_identity(&self) -> bool {
        self.glow_amount.abs() < 1e-4
            && self.halation_amount.abs() < 1e-4
            && self
                .tone_mapper
                .as_ref()
                .map_or(true, |tm| tm.is_empty() || tm == "none")
    }

    #[inline]
    pub fn has_glow(&self) -> bool {
        self.glow_amount.abs() >= 1e-4
    }

    #[inline]
    pub fn has_halation(&self) -> bool {
        self.halation_amount.abs() >= 1e-4
    }

    #[inline]
    pub fn has_tone_mapper(&self) -> bool {
        self.tone_mapper.as_deref() == Some("aces")
    }
}

/// Narkowicz ACES filmic tone mapping curve (CC0 / Public Domain).
///
/// Compresses linear dynamic range [0.0, ∞) smoothly to [0.0, 1.0].
/// Direct unscaled fit: x = 0.18 maps to ~0.2669, lifting mid-tones slightly
/// while highlights smoothly roll off without harsh clipping.
#[inline(always)]
pub fn aces_narkowicz(x: f32) -> f32 {
    let x = x.max(0.0);
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    ((x * (a * x + b)) / (x * (c * x + d) + e)).clamp(0.0, 1.0)
}

/// Evaluates soft-knee highlight excess above threshold `t` for luminance `y`.
///
/// Returns 0.0 below `t - k`, linear slope `y - t` above `t + k`, and a quadratic
/// Hermite curve in between, avoiding harsh edge clipping.
#[inline(always)]
pub fn soft_knee_excess(y: f32, threshold: f32) -> f32 {
    let t = threshold.max(1e-4);
    let k = 0.25 * t;
    let x = y - t;
    if x <= -k {
        0.0
    } else if x >= k {
        x
    } else {
        (x + k) * (x + k) / (4.0 * k)
    }
}

/// A compact RGB image buffer used for the reduced-resolution blur pyramid.
#[derive(Debug, Clone)]
pub struct ReducedRgbBuffer {
    pub width: usize,
    pub height: usize,
    pub data: Vec<f32>,
}

impl ReducedRgbBuffer {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            data: vec![0.0; width * height * 3],
        }
    }

    /// Bilinear sample with clamped edge coordinates.
    #[inline(always)]
    pub fn sample_bilinear(&self, u: f32, v: f32) -> [f32; 3] {
        let max_x = (self.width - 1) as i32;
        let max_y = (self.height - 1) as i32;

        let ix = (u.floor() as i32).clamp(0, max_x);
        let iy = (v.floor() as i32).clamp(0, max_y);
        let ix1 = (ix + 1).min(max_x);
        let iy1 = (iy + 1).min(max_y);

        let fx = (u - ix as f32).clamp(0.0, 1.0);
        let fy = (v - iy as f32).clamp(0.0, 1.0);

        let w00 = (1.0 - fx) * (1.0 - fy);
        let w10 = fx * (1.0 - fy);
        let w01 = (1.0 - fx) * fy;
        let w11 = fx * fy;

        let idx00 = (iy as usize * self.width + ix as usize) * 3;
        let idx10 = (iy as usize * self.width + ix1 as usize) * 3;
        let idx01 = (iy1 as usize * self.width + ix as usize) * 3;
        let idx11 = (iy1 as usize * self.width + ix1 as usize) * 3;

        let r = w00 * self.data[idx00]
            + w10 * self.data[idx10]
            + w01 * self.data[idx01]
            + w11 * self.data[idx11];
        let g = w00 * self.data[idx00 + 1]
            + w10 * self.data[idx10 + 1]
            + w01 * self.data[idx01 + 1]
            + w11 * self.data[idx11 + 1];
        let b = w00 * self.data[idx00 + 2]
            + w10 * self.data[idx10 + 2]
            + w01 * self.data[idx01 + 2]
            + w11 * self.data[idx11 + 2];

        [r, g, b]
    }

    /// Returns a row sampler for the given vertical coordinate `v`.
    #[inline(always)]
    pub fn row_sampler(&self, v: f32) -> BilinearRowSampler<'_> {
        BilinearRowSampler::new(self, v)
    }
}

/// Row sampler that hoists vertical bilinear weights outside the inner pixel loop.
pub struct BilinearRowSampler<'a> {
    buf: &'a ReducedRgbBuffer,
    row0_offset: usize,
    row1_offset: usize,
    fy: f32,
    max_x: i32,
}

impl<'a> BilinearRowSampler<'a> {
    #[inline(always)]
    pub fn new(buf: &'a ReducedRgbBuffer, v: f32) -> Self {
        let max_y = (buf.height - 1) as i32;
        let iy = (v.floor() as i32).clamp(0, max_y);
        let iy1 = (iy + 1).min(max_y);
        let fy = (v - iy as f32).clamp(0.0, 1.0);
        let row0_offset = iy as usize * buf.width * 3;
        let row1_offset = iy1 as usize * buf.width * 3;
        let max_x = (buf.width - 1) as i32;

        Self {
            buf,
            row0_offset,
            row1_offset,
            fy,
            max_x,
        }
    }

    #[inline(always)]
    pub fn sample(&self, u: f32) -> [f32; 3] {
        let ix = (u.floor() as i32).clamp(0, self.max_x);
        let ix1 = (ix + 1).min(self.max_x);
        let fx = (u - ix as f32).clamp(0.0, 1.0);

        let i0 = ix as usize * 3;
        let i1 = ix1 as usize * 3;

        let d = &self.buf.data;
        let r00 = d[self.row0_offset + i0];
        let r10 = d[self.row0_offset + i1];
        let r01 = d[self.row1_offset + i0];
        let r11 = d[self.row1_offset + i1];

        let g00 = d[self.row0_offset + i0 + 1];
        let g10 = d[self.row0_offset + i1 + 1];
        let g01 = d[self.row1_offset + i0 + 1];
        let g11 = d[self.row1_offset + i1 + 1];

        let b00 = d[self.row0_offset + i0 + 2];
        let b10 = d[self.row0_offset + i1 + 2];
        let b01 = d[self.row1_offset + i0 + 2];
        let b11 = d[self.row1_offset + i1 + 2];

        let top_r = r00 + fx * (r10 - r00);
        let bot_r = r01 + fx * (r11 - r01);
        let r = top_r + self.fy * (bot_r - top_r);

        let top_g = g00 + fx * (g10 - g00);
        let bot_g = g01 + fx * (g11 - g01);
        let g = top_g + self.fy * (bot_g - top_g);

        let top_b = b00 + fx * (b10 - b00);
        let bot_b = b01 + fx * (b11 - b01);
        let b = top_b + self.fy * (bot_b - top_b);

        [r, g, b]
    }
}

/// Downsamples a buffer by 2x using a 4-tap box filter.
fn downsample_2x(src: &ReducedRgbBuffer) -> ReducedRgbBuffer {
    let dst_w = (src.width / 2).max(1);
    let dst_h = (src.height / 2).max(1);
    let mut dst = ReducedRgbBuffer::new(dst_w, dst_h);

    let src_w = src.width;
    let src_h = src.height;

    for dy in 0..dst_h {
        let sy0 = (dy * 2).min(src_h - 1);
        let sy1 = (dy * 2 + 1).min(src_h - 1);

        let dst_row_idx = dy * dst_w * 3;
        let src_row0_idx = sy0 * src_w * 3;
        let src_row1_idx = sy1 * src_w * 3;

        for dx in 0..dst_w {
            let sx0 = (dx * 2).min(src_w - 1);
            let sx1 = (dx * 2 + 1).min(src_w - 1);

            let i00 = src_row0_idx + sx0 * 3;
            let i10 = src_row0_idx + sx1 * 3;
            let i01 = src_row1_idx + sx0 * 3;
            let i11 = src_row1_idx + sx1 * 3;

            let out_i = dst_row_idx + dx * 3;
            dst.data[out_i] =
                0.25 * (src.data[i00] + src.data[i10] + src.data[i01] + src.data[i11]);
            dst.data[out_i + 1] = 0.25
                * (src.data[i00 + 1] + src.data[i10 + 1] + src.data[i01 + 1] + src.data[i11 + 1]);
            dst.data[out_i + 2] = 0.25
                * (src.data[i00 + 2] + src.data[i10 + 2] + src.data[i01 + 2] + src.data[i11 + 2]);
        }
    }

    dst
}

/// Upsamples `src` by 2x into `dst` and accumulates with weight `weight`.
fn upsample_accumulate_2x(src: &ReducedRgbBuffer, dst: &mut ReducedRgbBuffer, weight: f32) {
    if weight <= 1e-5 {
        return;
    }

    let dst_w = dst.width;
    let dst_h = dst.height;

    let inv_scale_x = (src.width as f32) / (dst_w as f32);
    let inv_scale_y = (src.height as f32) / (dst_h as f32);

    for dy in 0..dst_h {
        let v = (dy as f32 + 0.5) * inv_scale_y - 0.5;
        let sampler = src.row_sampler(v);
        let dst_row_idx = dy * dst_w * 3;

        for dx in 0..dst_w {
            let u = (dx as f32 + 0.5) * inv_scale_x - 0.5;
            let s = sampler.sample(u);
            let out_i = dst_row_idx + dx * 3;

            dst.data[out_i] += s[0] * weight;
            dst.data[out_i + 1] += s[1] * weight;
            dst.data[out_i + 2] += s[2] * weight;
        }
    }
}

/// Runs Marius Bjørge's dual-filter multi-scale blur pyramid on `buf` for spread `radius` [0.0, 100.0].
fn run_pyramid_blur(buf: &ReducedRgbBuffer, radius: f32) -> ReducedRgbBuffer {
    let rad = (radius / 100.0).clamp(0.01, 1.0);

    // Build up to 4 downsampled levels
    let l1 = downsample_2x(buf);
    let l2 = downsample_2x(&l1);
    let l3 = downsample_2x(&l2);
    let l4 = downsample_2x(&l3);

    // Progressive weights for the pyramid levels based on radius
    let w4 = (rad * 4.0 - 3.0).clamp(0.0, 1.0);
    let w3 = (rad * 4.0 - 2.0).clamp(0.0, 1.0);
    let w2 = (rad * 4.0 - 1.0).clamp(0.0, 1.0);
    let w1 = (rad * 4.0).clamp(0.1, 1.0);

    let mut out_l3 = l3;
    upsample_accumulate_2x(&l4, &mut out_l3, w4);

    let mut out_l2 = l2;
    upsample_accumulate_2x(&out_l3, &mut out_l2, w3);

    let mut out_l1 = l1;
    upsample_accumulate_2x(&out_l2, &mut out_l1, w2);

    let mut out_l0 = ReducedRgbBuffer::new(buf.width, buf.height);
    upsample_accumulate_2x(&out_l1, &mut out_l0, w1);

    // Normalize accumulated energy so bloom spread does not arbitrarily blow out intensity
    let total_w = 1.0 + w1 + w2 * 0.75 + w3 * 0.5 + w4 * 0.25;
    let inv_w = 1.0 / total_w;
    for c in &mut out_l0.data {
        *c *= inv_w;
    }

    out_l0
}

/// Builds the pre-accumulated reduced bloom buffer with channel gains and tone recovery.
///
/// Scale factor is 1/8 of the linear frame dimensions.
pub fn build_combined_bloom_with_gains(
    linear: &LinearBuffer,
    looks: &LookEffects,
    gain_r: f32,
    gain_g: f32,
    gain_b: f32,
    h_val: f32,
    w_val: f32,
) -> Option<ReducedRgbBuffer> {
    let has_glow = looks.has_glow();
    let has_halation = looks.has_halation();

    if !has_glow && !has_halation {
        return None;
    }

    // Downsample factor: 1/8 linear size (per ED-16 brief)
    let w = linear.width as usize;
    let h = linear.height as usize;
    let red_w = (w / 8).max(1);
    let red_h = (h / 8).max(1);

    let glow_thresh = looks.glow_threshold / 100.0;
    let glow_gain = looks.glow_amount / 100.0;
    let hal_thresh = looks.halation_threshold / 100.0;
    let hal_gain = looks.halation_amount / 100.0;

    let mut glow_in = if has_glow {
        Some(ReducedRgbBuffer::new(red_w, red_h))
    } else {
        None
    };

    let mut hal_in = if has_halation {
        Some(ReducedRgbBuffer::new(red_w, red_h))
    } else {
        None
    };

    // Stride over the full linear frame, sampling 2x2 cells in each block
    let scale_x = (w as f32) / (red_w as f32);
    let scale_y = (h as f32) / (red_h as f32);

    for ry in 0..red_h {
        let base_y = ((ry as f32 + 0.5) * scale_y) as usize;
        let y0 = base_y.min(h - 1);
        let y1 = (base_y + 1).min(h - 1);

        let row_idx = ry * red_w * 3;

        for rx in 0..red_w {
            let base_x = ((rx as f32 + 0.5) * scale_x) as usize;
            let x0 = base_x.min(w - 1);
            let x1 = (base_x + 1).min(w - 1);

            let idx00 = (y0 * w + x0) * 3;
            let idx10 = (y0 * w + x1) * 3;
            let idx01 = (y1 * w + x0) * 3;
            let idx11 = (y1 * w + x1) * 3;

            let mut r = 0.25
                * (linear.data[idx00]
                    + linear.data[idx10]
                    + linear.data[idx01]
                    + linear.data[idx11])
                * gain_r;
            let mut g = 0.25
                * (linear.data[idx00 + 1]
                    + linear.data[idx10 + 1]
                    + linear.data[idx01 + 1]
                    + linear.data[idx11 + 1])
                * gain_g;
            let mut b = 0.25
                * (linear.data[idx00 + 2]
                    + linear.data[idx10 + 2]
                    + linear.data[idx01 + 2]
                    + linear.data[idx11 + 2])
                * gain_b;

            let mut y_luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;

            if y_luma >= 0.18 && (h_val != 0.0 || w_val != 0.0) {
                let w_h = if y_luma >= 0.65 {
                    1.0
                } else {
                    let t = (y_luma - 0.18) / (0.65 - 0.18);
                    t * t * (3.0 - 2.0 * t)
                };
                let w_w = if y_luma >= 1.0 {
                    1.0
                } else {
                    let t = (y_luma - 0.18) / (1.0 - 0.18);
                    t * t * (3.0 - 2.0 * t)
                };
                let delta_ev = w_h * h_val + w_w * w_val;
                if delta_ev != 0.0 {
                    let factor = delta_ev.exp2();
                    r *= factor;
                    g *= factor;
                    b *= factor;
                    y_luma *= factor;
                }
            }

            let out_i = row_idx + rx * 3;

            if let Some(ref mut g_buf) = glow_in {
                let excess = soft_knee_excess(y_luma, glow_thresh);
                if excess > 0.0 && y_luma > 1e-5 {
                    let factor = (excess / y_luma) * glow_gain;
                    g_buf.data[out_i] = r * factor;
                    g_buf.data[out_i + 1] = g * factor;
                    g_buf.data[out_i + 2] = b * factor;
                }
            }

            if let Some(ref mut h_buf) = hal_in {
                let excess = soft_knee_excess(y_luma, hal_thresh);
                if excess > 0.0 {
                    let factor = excess * hal_gain;
                    h_buf.data[out_i] = HALATION_TINT[0] * factor;
                    h_buf.data[out_i + 1] = HALATION_TINT[1] * factor;
                    h_buf.data[out_i + 2] = HALATION_TINT[2] * factor;
                }
            }
        }
    }

    let glow_bloom = glow_in.map(|buf| run_pyramid_blur(&buf, looks.glow_radius));
    let hal_bloom = hal_in.map(|buf| run_pyramid_blur(&buf, looks.halation_radius));

    match (glow_bloom, hal_bloom) {
        (Some(mut gb), Some(hb)) => {
            for (g_val, h_val) in gb.data.iter_mut().zip(hb.data.iter()) {
                *g_val += *h_val;
            }
            Some(gb)
        }
        (Some(gb), None) => Some(gb),
        (None, Some(hb)) => Some(hb),
        (None, None) => None,
    }
}

/// Builds the pre-accumulated reduced bloom buffer (combining Glow and Halation).
///
/// Scale factor is 1/8 of the linear frame dimensions.
pub fn build_combined_bloom(
    linear: &LinearBuffer,
    looks: &LookEffects,
) -> Option<ReducedRgbBuffer> {
    build_combined_bloom_with_gains(linear, looks, 1.0, 1.0, 1.0, 0.0, 0.0)
}

/// Applies glow, halation, and optional ACES tone mapping to a `LinearBuffer` in-place.
pub fn apply_looks_linear(linear: &mut LinearBuffer, looks: &LookEffects) {
    if looks.is_identity() {
        return;
    }

    if let Some(bloom) = build_combined_bloom(linear, looks) {
        let w = linear.width;
        let red_w = bloom.width;
        let red_h = bloom.height;

        let inv_scale_x = (red_w as f32) / (w as f32);
        let inv_scale_y = (red_h as f32) / (linear.height as f32);

        linear
            .data
            .par_chunks_exact_mut((w * 3) as usize)
            .enumerate()
            .for_each(|(y, row)| {
                let v = (y as f32 + 0.5) * inv_scale_y - 0.5;
                let sampler = bloom.row_sampler(v);
                for (x, px) in row.chunks_exact_mut(3).enumerate() {
                    let u = (x as f32 + 0.5) * inv_scale_x - 0.5;
                    let b = sampler.sample(u);
                    px[0] += b[0];
                    px[1] += b[1];
                    px[2] += b[2];
                }
            });
    }

    if looks.has_tone_mapper() {
        linear.data.par_chunks_exact_mut(3).for_each(|px| {
            px[0] = aces_narkowicz(px[0]);
            px[1] = aces_narkowicz(px[1]);
            px[2] = aces_narkowicz(px[2]);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_default_is_identity() {
        let looks = LookEffects::default();
        assert!(looks.is_identity());
        assert!(!looks.has_glow());
        assert!(!looks.has_halation());
        assert!(!looks.has_tone_mapper());
    }

    #[test]
    fn aces_narkowicz_monotonicity() {
        let mut prev = 0.0f32;
        for i in 0..1000 {
            let x = i as f32 * 0.05;
            let y = aces_narkowicz(x);
            assert!(y >= prev, "Inversion at x={x}: prev={prev}, y={y}");
            assert!(y <= 1.0, "ACES output exceeded 1.0 at x={x}: y={y}");
            prev = y;
        }
    }

    #[test]
    fn aces_narkowicz_mid_grey_value() {
        let y = aces_narkowicz(0.18);
        assert!(
            (y - 0.2669).abs() < 0.005,
            "Mid-grey 0.18 should map to ~0.2669, got {y}"
        );
    }
}
