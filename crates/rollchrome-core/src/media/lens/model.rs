//! Lensfun's calibration models, interpolation between calibrations, and the correction itself (DV-9).
//!
//! The models are the ones lensfun publishes its calibrations in, with its parameter names, so a
//! database entry is used as it stands:
//!
//! | | Model | Radius 1 is |
//! |---|---|---|
//! | Distortion | `ptlens` r_d = r (a r³ + b r² + c r + 1 − a − b − c); `poly3`; `poly5` | half the shorter side |
//! | Lateral CA | r_c = r (v + c r + b r²), red and blue against green | half the shorter side |
//! | Vignetting | `pa`: the light that arrived, 1 + k1 r² + k2 r⁴ + k3 r⁶ | half the diagonal |
//!
//! Each model maps where a point *should* be (undistorted) to where its light *landed*, which is what
//! a resample needs: for every output pixel, look up the source.

use rayon::prelude::*;

/// A distortion model, in units of half the calibration sensor's shorter side.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Distortion {
    Ptlens { a: f32, b: f32, c: f32 },
    Poly3 { k1: f32 },
    Poly5 { k1: f32, k2: f32 },
}

impl Distortion {
    /// The factor a radius is multiplied by: `r_d = r · factor(r)`.
    #[inline]
    pub fn factor(&self, r: f32) -> f32 {
        let r2 = r * r;
        match *self {
            Distortion::Ptlens { a, b, c } => a * r2 * r + b * r2 + c * r + 1.0 - a - b - c,
            Distortion::Poly3 { k1 } => 1.0 - k1 + k1 * r2,
            Distortion::Poly5 { k1, k2 } => 1.0 + k1 * r2 + k2 * r2 * r2,
        }
    }

    /// Linear interpolation between two calibrations of the same model; `None` when they differ.
    fn lerp(&self, other: &Self, t: f32) -> Option<Self> {
        let l = |a: f32, b: f32| a + (b - a) * t;
        match (*self, *other) {
            (
                Distortion::Ptlens { a, b, c },
                Distortion::Ptlens {
                    a: a2,
                    b: b2,
                    c: c2,
                },
            ) => Some(Distortion::Ptlens {
                a: l(a, a2),
                b: l(b, b2),
                c: l(c, c2),
            }),
            (Distortion::Poly3 { k1 }, Distortion::Poly3 { k1: k2 }) => {
                Some(Distortion::Poly3 { k1: l(k1, k2) })
            }
            (Distortion::Poly5 { k1, k2 }, Distortion::Poly5 { k1: j1, k2: j2 }) => {
                Some(Distortion::Poly5 {
                    k1: l(k1, j1),
                    k2: l(k2, j2),
                })
            }
            _ => None,
        }
    }
}

/// Lateral chromatic aberration: red and blue each a radial scaling of green, `[v, c, b]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tca {
    pub red: [f32; 3],
    pub blue: [f32; 3],
}

impl Tca {
    #[inline]
    fn factor(k: [f32; 3], r: f32) -> f32 {
        k[0] + k[1] * r + k[2] * r * r
    }

    fn lerp(&self, other: &Self, t: f32) -> Self {
        let l = |a: [f32; 3], b: [f32; 3]| {
            [
                a[0] + (b[0] - a[0]) * t,
                a[1] + (b[1] - a[1]) * t,
                a[2] + (b[2] - a[2]) * t,
            ]
        };
        Tca {
            red: l(self.red, other.red),
            blue: l(self.blue, other.blue),
        }
    }
}

/// One vignetting calibration: at a focal length, an f-number and a focus distance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VignettingCal {
    pub focal: f32,
    pub aperture: f32,
    pub distance: f32,
    pub k: [f32; 3],
}

/// The fall-off at a radius in half diagonals: what a pixel there recorded of the scene, 1 at the
/// centre. Held at radius 1: a sixth-order fit is a fit only out to the corner it was measured to, and
/// many bend back up past it (greycard's lens notes make the same point).
#[inline]
pub fn falloff(k: [f32; 3], r: f32) -> f32 {
    let r2 = r.min(1.0).powi(2);
    1.0 + k[0] * r2 + k[1] * r2 * r2 + k[2] * r2 * r2 * r2
}

/// Interpolates calibrations made at several focal lengths, linearly, held at the ends.
fn at_focal<T: Copy>(
    cals: &[(f32, T)],
    focal: f32,
    lerp: impl Fn(&T, &T, f32) -> Option<T>,
) -> Option<T> {
    let mut sorted: Vec<(f32, T)> = cals.to_vec();
    sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
    let first = *sorted.first()?;
    let last = *sorted.last()?;
    if focal <= first.0 {
        return Some(first.1);
    }
    if focal >= last.0 {
        return Some(last.1);
    }
    let i = sorted.iter().position(|(f, _)| *f >= focal)?;
    let (f0, v0) = sorted[i - 1];
    let (f1, v1) = sorted[i];
    if (f1 - f0).abs() < f32::EPSILON {
        return Some(v1);
    }
    let t = (focal - f0) / (f1 - f0);
    // Two neighbours in different models cannot be blended; the nearer one is used.
    Some(lerp(&v0, &v1, t).unwrap_or(if t < 0.5 { v0 } else { v1 }))
}

pub fn distortion_at(cals: &[(f32, Distortion)], focal: f32) -> Option<Distortion> {
    at_focal(cals, focal, |a, b, t| a.lerp(b, t))
}

pub fn tca_at(cals: &[(f32, Tca)], focal: f32) -> Option<Tca> {
    at_focal(cals, focal, |a, b, t| Some(a.lerp(b, t)))
}

/// The vignetting at a focal length and f-number: per focal length, interpolated in f-number at the
/// largest focus distance calibrated there (focus at infinity, the common case), then interpolated in
/// focal length.
pub fn vignetting_at(cals: &[VignettingCal], focal: f32, aperture: f32) -> Option<[f32; 3]> {
    let mut focals: Vec<f32> = cals.iter().map(|c| c.focal).collect();
    focals.sort_by(f32::total_cmp);
    focals.dedup();
    let per_focal: Vec<(f32, [f32; 3])> = focals
        .iter()
        .filter_map(|&f| {
            let here: Vec<&VignettingCal> = cals.iter().filter(|c| c.focal == f).collect();
            let far = here.iter().map(|c| c.distance).fold(f32::MIN, f32::max);
            let by_aperture: Vec<(f32, [f32; 3])> = here
                .iter()
                .filter(|c| c.distance == far)
                .map(|c| (c.aperture, c.k))
                .collect();
            at_focal(&by_aperture, aperture, |a, b, t| {
                Some([
                    a[0] + (b[0] - a[0]) * t,
                    a[1] + (b[1] - a[1]) * t,
                    a[2] + (b[2] - a[2]) * t,
                ])
            })
            .map(|k| (f, k))
        })
        .collect();
    at_focal(&per_focal, focal, |a, b, t| {
        Some([
            a[0] + (b[0] - a[0]) * t,
            a[1] + (b[1] - a[1]) * t,
            a[2] + (b[2] - a[2]) * t,
        ])
    })
}

/// A correction for one picture, in its own units.
#[derive(Debug, Clone, PartialEq)]
pub struct Correction {
    pub distortion: Option<Distortion>,
    pub tca: Option<Tca>,
    pub vignetting: Option<[f32; 3]>,
    /// A radius in half shorter sides of this picture, in the calibration's half shorter sides.
    pub radius_scale: f32,
    /// A radius in half diagonals of this picture, in the calibration's half diagonals.
    pub vignetting_scale: f32,
}

impl Correction {
    fn moves_pixels(&self) -> bool {
        self.distortion.is_some() || self.tca.is_some()
    }

    /// Where green's light for an undistorted point landed, in picture units.
    #[inline]
    fn green(&self, x: f32, y: f32) -> (f32, f32) {
        match self.distortion {
            None => (x, y),
            Some(d) => {
                let r = (x * x + y * y).sqrt() * self.radius_scale;
                let f = d.factor(r);
                (x * f, y * f)
            }
        }
    }

    /// Where red's (`channel` 0) or blue's (2) light landed, given where green's did.
    #[inline]
    fn channel(&self, channel: usize, (x, y): (f32, f32)) -> (f32, f32) {
        let Some(t) = self.tca else { return (x, y) };
        let k = if channel == 0 { t.red } else { t.blue };
        let r = (x * x + y * y).sqrt() * self.radius_scale;
        let f = Tca::factor(k, r);
        (x * f, y * f)
    }
}

/// The largest magnification at which every output pixel still samples inside the source: the
/// corrected picture fills the frame with no empty edge.
fn fill_scale(c: &Correction, half_w: f32, half_h: f32) -> f32 {
    let inside = |k: f32| {
        let steps = 64;
        (0..=steps).all(|i| {
            let s = i as f32 / steps as f32;
            [
                (-half_w + 2.0 * half_w * s, -half_h),
                (-half_w + 2.0 * half_w * s, half_h),
                (-half_w, -half_h + 2.0 * half_h * s),
                (half_w, -half_h + 2.0 * half_h * s),
            ]
            .iter()
            .all(|&(x, y)| {
                let g = c.green(x * k, y * k);
                [g, c.channel(0, g), c.channel(2, g)]
                    .iter()
                    .all(|p| p.0.abs() <= half_w + 1e-4 && p.1.abs() <= half_h + 1e-4)
            })
        })
    };
    if !c.moves_pixels() {
        return 1.0;
    }
    let (mut lo, mut hi) = (0.25_f32, 4.0_f32);
    if !inside(lo) {
        return lo;
    }
    if inside(hi) {
        return hi;
    }
    for _ in 0..40 {
        let mid = 0.5 * (lo + hi);
        if inside(mid) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo
}

/// Bilinear sample of one channel at a pixel-centre coordinate, clamped to the frame.
#[inline]
fn sample(pixels: &[[f32; 3]], w: usize, h: usize, x: f32, y: f32, ch: usize) -> f32 {
    let x = x.clamp(0.0, (w - 1) as f32);
    let y = y.clamp(0.0, (h - 1) as f32);
    let x0 = x.floor() as usize;
    let y0 = y.floor() as usize;
    let x1 = (x0 + 1).min(w - 1);
    let y1 = (y0 + 1).min(h - 1);
    let fx = x - x0 as f32;
    let fy = y - y0 as f32;
    let p = |xx: usize, yy: usize| pixels[yy * w + xx][ch];
    let top = p(x0, y0) + (p(x1, y0) - p(x0, y0)) * fx;
    let bottom = p(x0, y1) + (p(x1, y1) - p(x0, y1)) * fx;
    top + (bottom - top) * fy
}

/// Where an output pixel's light for one channel comes from in the source, in source pixel
/// coordinates (pixel centres at integers), for a picture of `width` by `height` at magnification
/// `scale`. What [`apply`] samples, exposed so a test can check the claim about edges directly.
pub fn source_pixel(
    c: &Correction,
    width: u32,
    height: u32,
    scale: f32,
    x: f32,
    y: f32,
    channel: usize,
) -> (f32, f32) {
    let (w, h) = (width as f32, height as f32);
    let unit = w.min(h) * 0.5;
    let (half_w, half_h) = (w * 0.5 / unit, h * 0.5 / unit);
    let ox = (x + 0.5) / unit - half_w;
    let oy = (y + 0.5) / unit - half_h;
    let g = c.green(ox * scale, oy * scale);
    let p = if channel == 1 {
        g
    } else {
        c.channel(channel, g)
    };
    ((p.0 + half_w) * unit - 0.5, (p.1 + half_h) * unit - 0.5)
}

/// The magnification [`apply`] uses for a picture of `width` by `height`.
pub fn magnification(c: &Correction, width: u32, height: u32) -> f32 {
    let unit = (width.min(height) as f32) * 0.5;
    fill_scale(c, width as f32 * 0.5 / unit, height as f32 * 0.5 / unit)
}

/// Applies a correction to linear pixels in sensor orientation. Returns the corrected pixels and the
/// magnification used. One resample: each output pixel finds where each channel's light landed,
/// samples it, and divides by the vignetting gain there.
pub fn apply(width: u32, height: u32, pixels: &[[f32; 3]], c: &Correction) -> (Vec<[f32; 3]>, f32) {
    let (w, h) = (width as usize, height as usize);
    // Picture units: one is half the shorter side, the centre is the frame's centre.
    let unit = (w.min(h) as f32) * 0.5;
    let half_w = w as f32 * 0.5 / unit;
    let half_h = h as f32 * 0.5 / unit;
    let half_diag_units = (half_w * half_w + half_h * half_h).sqrt();
    let scale = fill_scale(c, half_w, half_h);

    let mut out = vec![[0.0f32; 3]; w * h];
    out.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (x, px) in row.iter_mut().enumerate() {
            for (ch, value) in px.iter_mut().enumerate() {
                let (sx, sy) = source_pixel(c, width, height, scale, x as f32, y as f32, ch);
                let mut v = sample(pixels, w, h, sx, sy, ch);
                if let Some(k) = c.vignetting {
                    // Back to picture units, about the centre, for the radius.
                    let px_u = (sx + 0.5) / unit - half_w;
                    let py_u = (sy + 0.5) / unit - half_h;
                    let r =
                        (px_u * px_u + py_u * py_u).sqrt() / half_diag_units * c.vignetting_scale;
                    let gain = falloff(k, r);
                    if gain > 0.05 {
                        v /= gain;
                    }
                }
                *value = v;
            }
        }
    });
    (out, scale)
}
