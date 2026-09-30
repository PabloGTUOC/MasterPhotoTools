//! Monotone tone curves and cubic spline interpolation (ED-10).
//!
//! Citations:
//! - F. N. Fritsch and R. E. Carlson (1980), "Monotone Piecewise Cubic Interpolation",
//!   SIAM Journal on Numerical Analysis, Vol. 17, No. 2, pp. 238–246.
//!   https://doi.org/10.1137/0717021
//!
//! Tone curves are evaluated in display-encoded space [0.0, 1.0].
//! Curves are represented by user control points. Fritsch–Carlson piecewise cubic Hermite
//! interpolation guarantees monotonic response with zero overshoot, oscillations, or gradient
//! inversions. Endpoints (0.0, 0.0) and (1.0, 1.0) are fixed.

use serde::{Deserialize, Serialize};

/// Size of the 1D curve lookup table evaluated once per recipe change.
pub const CURVE_LUT_SIZE: usize = 1024;

/// A single control point on a tone curve in [0.0, 1.0] x [0.0, 1.0].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CurvePoint {
    pub x: f32,
    pub y: f32,
}

impl CurvePoint {
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// Tone curves for luminance (luma/master) and individual RGB color channels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ToneCurves {
    #[serde(default)]
    pub luma: Vec<CurvePoint>,
    #[serde(default)]
    pub red: Vec<CurvePoint>,
    #[serde(default)]
    pub green: Vec<CurvePoint>,
    #[serde(default)]
    pub blue: Vec<CurvePoint>,
}

impl ToneCurves {
    /// Returns true if all curves are empty or equivalent to identity.
    pub fn is_identity(&self) -> bool {
        is_channel_identity(&self.luma)
            && is_channel_identity(&self.red)
            && is_channel_identity(&self.green)
            && is_channel_identity(&self.blue)
    }
}

/// Checks if a list of curve points is an identity curve.
pub fn is_channel_identity(points: &[CurvePoint]) -> bool {
    if points.is_empty() {
        return true;
    }
    // If only endpoints (0,0) and (1,1)
    if points.len() == 2
        && (points[0].x - 0.0).abs() < 1e-5
        && (points[0].y - 0.0).abs() < 1e-5
        && (points[1].x - 1.0).abs() < 1e-5
        && (points[1].y - 1.0).abs() < 1e-5
    {
        return true;
    }
    // All points lie on y = x
    points.iter().all(|p| (p.x - p.y).abs() < 1e-5)
}

/// A 1D lookup table for a tone curve evaluated at `CURVE_LUT_SIZE` uniform samples.
#[derive(Debug, Clone)]
pub struct CurveTable {
    pub table: [f32; CURVE_LUT_SIZE],
    pub is_identity: bool,
}

impl Default for CurveTable {
    fn default() -> Self {
        Self::identity()
    }
}

impl CurveTable {
    /// Creates an exact identity curve table y = x.
    pub fn identity() -> Self {
        let mut table = [0.0f32; CURVE_LUT_SIZE];
        for (i, val) in table.iter_mut().enumerate() {
            *val = i as f32 / (CURVE_LUT_SIZE - 1) as f32;
        }
        Self {
            table,
            is_identity: true,
        }
    }

    /// Evaluates control points into a `CurveTable` using Fritsch–Carlson monotone cubic interpolation.
    pub fn from_points(points: &[CurvePoint]) -> Self {
        if is_channel_identity(points) {
            return Self::identity();
        }

        let spline = MonotoneSpline::from_points(points);
        let mut table = [0.0f32; CURVE_LUT_SIZE];
        for (i, val) in table.iter_mut().enumerate() {
            let x = i as f32 / (CURVE_LUT_SIZE - 1) as f32;
            *val = spline.evaluate(x);
        }
        Self {
            table,
            is_identity: false,
        }
    }

    /// Returns true if this table is identity (no-op).
    #[inline(always)]
    pub fn is_identity(&self) -> bool {
        self.is_identity
    }

    /// Samples the curve table at display value `x` in [0.0, 1.0] with linear interpolation.
    #[inline(always)]
    pub fn sample(&self, x: f32) -> f32 {
        if self.is_identity {
            return x.clamp(0.0, 1.0);
        }
        let clamped = x.clamp(0.0, 1.0);
        self.sample_unit(clamped)
    }

    /// Samples when `x` is already known to be in [0.0, 1.0].
    #[inline(always)]
    pub fn sample_unit(&self, x: f32) -> f32 {
        let pos = x * (CURVE_LUT_SIZE - 1) as f32;
        let idx = (pos as usize).min(CURVE_LUT_SIZE - 2);
        let frac = pos - idx as f32;
        unsafe {
            let v0 = *self.table.get_unchecked(idx);
            let v1 = *self.table.get_unchecked(idx + 1);
            v0 + (v1 - v0) * frac
        }
    }
}

/// Evaluated lookup tables for Luma, Red, Green, and Blue tone curves.
#[derive(Debug, Clone, Default)]
pub struct ToneCurvesTable {
    pub luma: CurveTable,
    pub red: CurveTable,
    pub green: CurveTable,
    pub blue: CurveTable,
    pub has_luma: bool,
    pub has_red: bool,
    pub has_green: bool,
    pub has_blue: bool,
    pub has_any: bool,
}

impl ToneCurvesTable {
    /// Compiles `ToneCurves` into lookup tables evaluated once per recipe change.
    pub fn from_recipe(curves: Option<&ToneCurves>) -> Self {
        let curves = match curves {
            Some(c) if !c.is_identity() => c,
            _ => {
                return Self {
                    luma: CurveTable::identity(),
                    red: CurveTable::identity(),
                    green: CurveTable::identity(),
                    blue: CurveTable::identity(),
                    has_luma: false,
                    has_red: false,
                    has_green: false,
                    has_blue: false,
                    has_any: false,
                };
            }
        };

        let luma = CurveTable::from_points(&curves.luma);
        let red = CurveTable::from_points(&curves.red);
        let green = CurveTable::from_points(&curves.green);
        let blue = CurveTable::from_points(&curves.blue);

        let has_luma = !luma.is_identity;
        let has_red = !red.is_identity;
        let has_green = !green.is_identity;
        let has_blue = !blue.is_identity;
        let has_any = has_luma || has_red || has_green || has_blue;

        Self {
            luma,
            red,
            green,
            blue,
            has_luma,
            has_red,
            has_green,
            has_blue,
            has_any,
        }
    }

    /// Applies luma and RGB curves to a display-encoded pixel (r, g, b in [0.0, 1.0]).
    ///
    /// Luma curve scales channels proportionally to preserve chromaticity ratios (r/Y, g/Y, b/Y).
    /// RGB curves shift individual channels independently.
    #[inline(always)]
    pub fn apply(&self, r: f32, g: f32, b: f32) -> (f32, f32, f32) {
        if !self.has_any {
            return (r, g, b);
        }

        // 1. Luma curve: preserves chromaticity ratios
        let (lr, lg, lb) = if self.has_luma {
            let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
            if y > 1e-7 {
                let y_curved = self.luma.sample_unit(y.min(1.0));
                let scale = y_curved / y;
                (
                    (r * scale).min(1.0),
                    (g * scale).min(1.0),
                    (b * scale).min(1.0),
                )
            } else {
                let y_curved = self.luma.table[0];
                (y_curved, y_curved, y_curved)
            }
        } else {
            (r, g, b)
        };

        // 2. RGB curves: shift individual channels independently
        let final_r = if self.has_red {
            self.red.sample_unit(lr)
        } else {
            lr
        };
        let final_g = if self.has_green {
            self.green.sample_unit(lg)
        } else {
            lg
        };
        let final_b = if self.has_blue {
            self.blue.sample_unit(lb)
        } else {
            lb
        };

        (final_r, final_g, final_b)
    }
}

/// Fritsch–Carlson monotone piecewise cubic Hermite interpolator.
#[derive(Debug, Clone)]
pub struct MonotoneSpline {
    xs: Vec<f32>,
    ys: Vec<f32>,
    tangents: Vec<f32>,
}

impl MonotoneSpline {
    /// Constructs a monotone spline from control points.
    ///
    /// Endpoints (0.0, 0.0) and (1.0, 1.0) are guaranteed.
    /// Unordered points are sorted by x, duplicates are merged, and coordinates are clamped to [0.0, 1.0].
    pub fn from_points(raw_points: &[CurvePoint]) -> Self {
        // Collect, clamp, and sort points
        let mut pts: Vec<CurvePoint> = raw_points
            .iter()
            .map(|p| CurvePoint {
                x: p.x.clamp(0.0, 1.0),
                y: p.y.clamp(0.0, 1.0),
            })
            .collect();

        pts.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));

        // Deduplicate points with nearly identical x coordinates
        let mut deduped: Vec<CurvePoint> = Vec::with_capacity(pts.len() + 2);
        for p in pts {
            if let Some(last) = deduped.last_mut() {
                if (p.x - last.x).abs() < 1e-5 {
                    // Average the y values for duplicate x
                    last.y = (last.y + p.y) * 0.5;
                    continue;
                }
            }
            deduped.push(p);
        }

        // Ensure fixed endpoints (0.0, 0.0) and (1.0, 1.0)
        if deduped.is_empty() {
            deduped.push(CurvePoint::new(0.0, 0.0));
            deduped.push(CurvePoint::new(1.0, 1.0));
        } else {
            if deduped[0].x > 0.0 {
                deduped.insert(0, CurvePoint::new(0.0, 0.0));
            } else {
                deduped[0] = CurvePoint::new(0.0, 0.0);
            }

            let last_idx = deduped.len() - 1;
            if deduped[last_idx].x < 1.0 {
                deduped.push(CurvePoint::new(1.0, 1.0));
            } else {
                deduped[last_idx] = CurvePoint::new(1.0, 1.0);
            }
        }

        let n = deduped.len();
        let mut xs = Vec::with_capacity(n);
        let mut ys = Vec::with_capacity(n);
        for p in &deduped {
            xs.push(p.x);
            ys.push(p.y);
        }

        // Step 1: Secant slopes delta_k = (y_{k+1} - y_k) / (x_{k+1} - x_k)
        let mut deltas = Vec::with_capacity(n - 1);
        for k in 0..n - 1 {
            let h = xs[k + 1] - xs[k];
            let delta = if h > 1e-7 {
                (ys[k + 1] - ys[k]) / h
            } else {
                0.0
            };
            deltas.push(delta);
        }

        // Step 2: Initial tangents (standard arithmetic mean of secants)
        let mut tangents = vec![0.0f32; n];
        if n == 2 {
            tangents[0] = deltas[0];
            tangents[1] = deltas[0];
        } else {
            tangents[0] = deltas[0];
            tangents[n - 1] = deltas[n - 2];
            for k in 1..n - 1 {
                if deltas[k - 1] * deltas[k] <= 0.0 {
                    tangents[k] = 0.0;
                } else {
                    tangents[k] = 0.5 * (deltas[k - 1] + deltas[k]);
                }
            }
        }

        // Step 3: Fritsch–Carlson monotonicity condition
        // If delta_k == 0, tangents at both endpoints must be zero.
        // Otherwise, alpha = d_k / delta_k, beta = d_{k+1} / delta_k.
        // If alpha^2 + beta^2 > 9, scale back by tau = 3 / sqrt(alpha^2 + beta^2).
        for k in 0..n - 1 {
            let delta = deltas[k];
            if delta.abs() < 1e-7 {
                tangents[k] = 0.0;
                tangents[k + 1] = 0.0;
            } else {
                let alpha = tangents[k] / delta;
                let beta = tangents[k + 1] / delta;
                if alpha < 0.0 {
                    tangents[k] = 0.0;
                }
                if beta < 0.0 {
                    tangents[k + 1] = 0.0;
                }
                let alpha = tangents[k] / delta;
                let beta = tangents[k + 1] / delta;
                let sum_sq = alpha * alpha + beta * beta;
                if sum_sq > 9.0 {
                    let tau = 3.0 / sum_sq.sqrt();
                    tangents[k] = tau * alpha * delta;
                    tangents[k + 1] = tau * beta * delta;
                }
            }
        }

        Self { xs, ys, tangents }
    }

    /// Evaluates the cubic Hermite spline at `x` in [0.0, 1.0].
    pub fn evaluate(&self, x: f32) -> f32 {
        let n = self.xs.len();
        if n == 0 {
            return x.clamp(0.0, 1.0);
        }
        if x <= self.xs[0] {
            return self.ys[0];
        }
        if x >= self.xs[n - 1] {
            return self.ys[n - 1];
        }

        // Binary search to find interval [xs[k], xs[k+1]]
        let mut low = 0;
        let mut high = n - 1;
        while low < high - 1 {
            let mid = (low + high) / 2;
            if self.xs[mid] <= x {
                low = mid;
            } else {
                high = mid;
            }
        }
        let k = low;
        let h = self.xs[k + 1] - self.xs[k];
        if h < 1e-7 {
            return self.ys[k];
        }

        let t = (x - self.xs[k]) / h;
        let t2 = t * t;
        let t3 = t2 * t;

        // Hermite basis functions
        let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
        let h10 = t3 - 2.0 * t2 + t;
        let h01 = -2.0 * t3 + 3.0 * t2;
        let h11 = t3 - t2;

        let y = h00 * self.ys[k]
            + h10 * h * self.tangents[k]
            + h01 * self.ys[k + 1]
            + h11 * h * self.tangents[k + 1];

        y.clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_untouched_curve_is_exact_identity() {
        let curves = ToneCurves::default();
        assert!(curves.is_identity());
        let table = ToneCurvesTable::from_recipe(Some(&curves));
        assert!(!table.has_any);

        // Explicit identity points
        let identity_pts = vec![CurvePoint::new(0.0, 0.0), CurvePoint::new(1.0, 1.0)];
        let curve_table = CurveTable::from_points(&identity_pts);
        assert!(curve_table.is_identity());

        for i in 0..=100 {
            let x = i as f32 / 100.0;
            assert!((curve_table.sample(x) - x).abs() < 1e-6);
        }
    }

    #[test]
    fn curve_interpolation_is_strictly_monotonic_without_overshoot() {
        // Classic photographic S-curve
        let s_curve = vec![
            CurvePoint::new(0.0, 0.0),
            CurvePoint::new(0.25, 0.15),
            CurvePoint::new(0.75, 0.85),
            CurvePoint::new(1.0, 1.0),
        ];
        let spline = MonotoneSpline::from_points(&s_curve);
        let table = CurveTable::from_points(&s_curve);

        let mut prev_val = -1.0f32;
        for i in 0..=1000 {
            let x = i as f32 / 1000.0;
            let val = spline.evaluate(x);
            assert!(
                (0.0..=1.0).contains(&val),
                "overshoot: val {} out of bounds [0, 1] at x={}",
                val,
                x
            );
            assert!(
                val >= prev_val - 1e-6,
                "non-monotonic: val {} < prev {} at x={}",
                val,
                prev_val,
                x
            );
            prev_val = val;

            let sample_val = table.sample(x);
            assert!(
                (sample_val - val).abs() < 0.005,
                "table sample deviates from spline at x={}",
                x
            );
        }
    }

    #[test]
    fn curve_evaluation_handles_unordered_control_points_safely() {
        // Control points specified deliberately backwards and scrambled
        let scrambled = vec![
            CurvePoint::new(0.8, 0.9),
            CurvePoint::new(0.2, 0.1),
            CurvePoint::new(0.5, 0.5),
        ];
        let table = CurveTable::from_points(&scrambled);
        assert!(!table.is_identity());

        let mut prev = -1.0f32;
        for i in 0..=100 {
            let x = i as f32 / 100.0;
            let y = table.sample(x);
            assert!((0.0..=1.0).contains(&y));
            assert!(y >= prev - 1e-6);
            prev = y;
        }
    }

    #[test]
    fn luma_curve_preserves_chromaticity_ratios() {
        // Applying a luma S-curve must scale R, G, B proportionally,
        // leaving the chromaticity coordinates r / (r+g+b) and r / Y unchanged.
        let curves = ToneCurves {
            luma: vec![
                CurvePoint::new(0.0, 0.0),
                CurvePoint::new(0.3, 0.2),
                CurvePoint::new(0.7, 0.8),
                CurvePoint::new(1.0, 1.0),
            ],
            ..Default::default()
        };
        let table = ToneCurvesTable::from_recipe(Some(&curves));
        assert!(table.has_any);

        let colors = [(0.6f32, 0.3f32, 0.1f32), (0.2, 0.5, 0.8), (0.7, 0.7, 0.2)];

        for &(r, g, b) in &colors {
            let sum_in = r + g + b;
            let ratio_r_in = r / sum_in;
            let ratio_g_in = g / sum_in;
            let ratio_b_in = b / sum_in;

            let (cr, cg, cb) = table.apply(r, g, b);
            let sum_out = cr + cg + cb;
            let ratio_r_out = cr / sum_out;
            let ratio_g_out = cg / sum_out;
            let ratio_b_out = cb / sum_out;

            assert!(
                (ratio_r_out - ratio_r_in).abs() < 1e-5,
                "R chromaticity ratio changed: {} vs {}",
                ratio_r_out,
                ratio_r_in
            );
            assert!(
                (ratio_g_out - ratio_g_in).abs() < 1e-5,
                "G chromaticity ratio changed: {} vs {}",
                ratio_g_out,
                ratio_g_in
            );
            assert!(
                (ratio_b_out - ratio_b_in).abs() < 1e-5,
                "B chromaticity ratio changed: {} vs {}",
                ratio_b_out,
                ratio_b_in
            );
        }
    }

    #[test]
    fn rgb_curves_shift_individual_channels_independently() {
        // Boost Red curve while Green and Blue are identity
        let curves = ToneCurves {
            red: vec![
                CurvePoint::new(0.0, 0.0),
                CurvePoint::new(0.5, 0.75),
                CurvePoint::new(1.0, 1.0),
            ],
            ..Default::default()
        };
        let table = ToneCurvesTable::from_recipe(Some(&curves));

        let (r, g, b) = (0.5f32, 0.4f32, 0.3f32);
        let (out_r, out_g, out_b) = table.apply(r, g, b);

        assert!(
            out_r > r + 0.1,
            "Red channel was not boosted: in={}, out={}",
            r,
            out_r
        );
        assert!(
            (out_g - g).abs() < 1e-5,
            "Green channel shifted unexpectedly: in={}, out={}",
            g,
            out_g
        );
        assert!(
            (out_b - b).abs() < 1e-5,
            "Blue channel shifted unexpectedly: in={}, out={}",
            b,
            out_b
        );
    }
}
