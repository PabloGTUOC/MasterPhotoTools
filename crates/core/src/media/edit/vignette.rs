//! Scale-independent vignette in linear light (ED-14).
//!
//! ### Physical Model & Linear Light Domain
//! In physical optics, natural vignetting ($\cos^4$ law) and mechanical vignetting arise
//! from lens barrel obstruction and geometric foreshortening of the entrance pupil at oblique
//! angles. Because vignetting is a physical loss (or gain) of light entering the lens, it
//! operates as a smooth multiplicative scaling on radiant flux in **linear light** ($[0.0, \infty)$),
//! prior to tone curve shaping, non-linear colour grading, and display gamma encoding.
//!
//! Positive `amount` values ($> 0.0$) boost peripheral exposure (lightening corners, for instance
//! to correct optical fall-off), while negative values ($< 0.0$) attenuate peripheral exposure
//! (darkening corners for artistic isolation).
//!
//! ### Post-Geometry Placement
//! The vignette coordinate space is defined in normalized coordinates relative to the
//! **final frame after the geometry stage** (crop, rotate, straighten, flip). A photographer
//! who crops into a subregion expects the optical focus of the vignette to frame the retained
//! composition rather than the discarded borders.

use super::pipeline::LinearBuffer;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

fn default_fifty() -> f32 {
    50.0
}

/// Scale-independent vignette parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Vignette {
    /// Vignette strength from -100.0 (darkens corners) to +100.0 (lightens corners). Default: 0.0.
    #[serde(default)]
    pub amount: f32,
    /// Radial midpoint of the transition zone as a percentage from center (0.0) to corners (100.0). Default: 50.0.
    #[serde(default = "default_fifty")]
    pub midpoint: f32,
    /// Shape profile from -100.0 (rectangular/frame-bound) to 0.0 (oval/aspect-matching) to +100.0 (circular). Default: 0.0.
    #[serde(default)]
    pub roundness: f32,
    /// Softness of the transition edge as a percentage from 0.0 (hard edge) to 100.0 (broad feather). Default: 50.0.
    #[serde(default = "default_fifty")]
    pub feather: f32,
}

impl Default for Vignette {
    fn default() -> Self {
        Self {
            amount: 0.0,
            midpoint: 50.0,
            roundness: 0.0,
            feather: 50.0,
        }
    }
}

impl Vignette {
    /// Returns true if the vignette is at rest (amount == 0.0).
    pub fn is_identity(&self) -> bool {
        self.amount.abs() < 1e-4
    }

    /// Precomputes parameters for rapid per-pixel evaluation.
    pub fn compile(&self, width: u32, height: u32) -> CompiledVignette {
        let w = width.max(1) as f32;
        let h = height.max(1) as f32;
        let xc = w * 0.5;
        let yc = h * 0.5;
        let inv_hx = 1.0 / xc;
        let inv_hy = 1.0 / yc;

        let aspect = w / h;
        let d_max = std::f32::consts::SQRT_2; // ~1.41421356

        let circ_scale = if aspect >= 1.0 {
            (2.0 / (aspect * aspect + 1.0)).sqrt()
        } else {
            (2.0 / (1.0 + 1.0 / (aspect * aspect))).sqrt()
        };

        let m = (self.midpoint / 100.0).clamp(0.0, 1.0);
        let f = (self.feather / 100.0).clamp(0.0, 1.0);
        let d_mid = m * d_max;
        let w_trans = (0.01 + 0.99 * f) * 0.5 * d_max;
        let d0 = (d_mid - w_trans).max(0.0);
        let d1 = d_mid + w_trans;
        let inv_span = 1.0 / (d1 - d0).max(1e-5);

        let a = (self.amount / 100.0).clamp(-1.0, 1.0);
        let r_param = (self.roundness / 100.0).clamp(-1.0, 1.0);

        CompiledVignette {
            xc,
            yc,
            inv_hx,
            inv_hy,
            aspect,
            d_max,
            circ_scale,
            d0,
            inv_span,
            a,
            r_param,
            is_identity: self.is_identity(),
        }
    }
}

/// Precomputed vignette parameters.
#[derive(Debug, Clone, Copy)]
pub struct CompiledVignette {
    pub xc: f32,
    pub yc: f32,
    pub inv_hx: f32,
    pub inv_hy: f32,
    pub aspect: f32,
    pub d_max: f32,
    pub circ_scale: f32,
    pub d0: f32,
    pub inv_span: f32,
    pub a: f32,
    pub r_param: f32,
    pub is_identity: bool,
}

impl CompiledVignette {
    /// Computes the linear multiplicative gain at pixel coordinates `(x, y)`.
    #[inline(always)]
    pub fn gain(&self, x: f32, y: f32) -> f32 {
        if self.is_identity {
            return 1.0;
        }
        let v = (y + 0.5 - self.yc) * self.inv_hy;
        self.gain_with_v(x, v, v * v)
    }

    /// Computes the linear multiplicative gain with precomputed vertical coordinate `v` and `v^2`.
    #[inline(always)]
    pub fn gain_with_v(&self, x: f32, v: f32, v2: f32) -> f32 {
        if self.is_identity {
            return 1.0;
        }

        let u = (x + 0.5 - self.xc) * self.inv_hx;
        let u2 = u * u;
        let d_oval = (u2 + v2).sqrt();

        let d = if self.r_param > 0.0 {
            let circ_dist = if self.aspect >= 1.0 {
                ((u * self.aspect) * (u * self.aspect) + v2).sqrt() * self.circ_scale
            } else {
                (u2 + (v / self.aspect) * (v / self.aspect)).sqrt() * self.circ_scale
            };
            (1.0 - self.r_param) * d_oval + self.r_param * circ_dist
        } else if self.r_param < 0.0 {
            let d_rect = u.abs().max(v.abs()) * self.d_max;
            (1.0 + self.r_param) * d_oval + (-self.r_param) * d_rect
        } else {
            d_oval
        };

        let t = ((d - self.d0) * self.inv_span).clamp(0.0, 1.0);
        let weight = t * t * (3.0 - 2.0 * t);
        (1.0 + self.a * weight).max(0.0)
    }
}

/// Applies a vignette to a `LinearBuffer` in-place in linear light.
pub fn apply_vignette(buffer: &mut LinearBuffer, vignette: &Vignette) {
    if vignette.is_identity() {
        return;
    }

    let compiled = vignette.compile(buffer.width, buffer.height);
    let w = buffer.width;
    let row_len = (w * 3) as usize;

    buffer
        .data
        .par_chunks_exact_mut(row_len)
        .enumerate()
        .for_each(|(y, row)| {
            let y_f = y as f32;
            for (x, px) in row.chunks_exact_mut(3).enumerate() {
                let gain = compiled.gain(x as f32, y_f);
                px[0] *= gain;
                px[1] *= gain;
                px[2] *= gain;
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vignette_default_is_identity() {
        let v = Vignette::default();
        assert!(v.is_identity());
        let mut buf = LinearBuffer::new(10, 10);
        buf.data.fill(0.5);
        apply_vignette(&mut buf, &v);
        for &val in &buf.data {
            assert_eq!(val, 0.5);
        }
    }

    #[test]
    fn vignette_darkens_corners_and_preserves_center() {
        let v = Vignette {
            amount: -100.0,
            midpoint: 50.0,
            roundness: 0.0,
            feather: 50.0,
        };
        let compiled = v.compile(100, 100);
        // Center has no attenuation (gain = 1.0)
        let center_gain = compiled.gain(49.5, 49.5);
        assert!((center_gain - 1.0).abs() < 1e-3);

        // Extreme corner has full attenuation (gain = 0.0)
        let corner_gain = compiled.gain(0.0, 0.0);
        assert!(corner_gain < 0.05);
    }

    #[test]
    fn vignette_lightens_corners_with_positive_amount() {
        let v = Vignette {
            amount: 50.0,
            midpoint: 50.0,
            roundness: 0.0,
            feather: 50.0,
        };
        let compiled = v.compile(100, 100);
        let center_gain = compiled.gain(49.5, 49.5);
        assert!((center_gain - 1.0).abs() < 1e-3);

        let corner_gain = compiled.gain(0.0, 0.0);
        assert!(corner_gain > 1.4);
    }
}
