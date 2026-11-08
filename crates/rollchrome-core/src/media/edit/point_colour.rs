//! Point colour adjustments evaluated in OkLCh perceptual color space (RP-10).
//!
//! Allows picking a precise colour in the photograph and selectively adjusting its
//! hue, saturation, and luminance within a configurable OkLCh tolerance envelope.

use serde::{Deserialize, Serialize};

fn default_range() -> f32 {
    50.0
}

/// Point colour target and adjustments in OkLCh color space (RP-10).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PointColour {
    /// Target hue in OkLCh degrees [0.0, 360.0).
    pub hue: f32,
    /// Target chroma in OkLCh [0.0, ~0.4].
    pub chroma: f32,
    /// Target lightness in OkLCh [0.0, 1.0].
    pub lightness: f32,
    /// Selection range tolerance: 0.0 to 100.0 (default 50.0).
    #[serde(default = "default_range")]
    pub range: f32,
    /// Hue shift in degrees: -100.0 to +100.0 (default 0.0).
    #[serde(default)]
    pub hue_shift: f32,
    /// Saturation percentage scale: -100.0 to +100.0 (default 0.0).
    #[serde(default)]
    pub saturation: f32,
    /// Luminance percentage shift: -100.0 to +100.0 (default 0.0).
    #[serde(default)]
    pub luminance: f32,
}

impl PointColour {
    pub fn new(hue: f32, chroma: f32, lightness: f32) -> Self {
        Self {
            hue,
            chroma,
            lightness,
            range: 50.0,
            hue_shift: 0.0,
            saturation: 0.0,
            luminance: 0.0,
        }
    }

    /// A point at rest has all adjustment deltas at 0.0 and behaves as exact identity.
    pub fn is_identity(&self) -> bool {
        self.hue_shift == 0.0 && self.saturation == 0.0 && self.luminance == 0.0
    }
}

/// Result of sampling a 5×5 box on the unadjusted settle proxy (RP-10).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PickedColour {
    /// Hue angle in OkLCh degrees [0.0, 360.0).
    pub hue: f32,
    /// Chroma in OkLCh.
    pub chroma: f32,
    /// Lightness in OkLCh [0.0, 1.0].
    pub lightness: f32,
}

/// Pre-scaled active point colour for multi-million pixel passes.
#[derive(Debug, Clone)]
struct CompiledPoint {
    h0: f32,
    c0: f32,
    l0: f32,
    w_h: f32,
    w_c: f32,
    w_l: f32,
    hue_shift_scaled: f32,
    sat_scaled: f32,
    lum_scaled: f32,
}

/// Compiled collection of point colour adjustments.
pub struct CompiledPointColours {
    points: Vec<CompiledPoint>,
}

#[inline(always)]
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl CompiledPointColours {
    /// Compiles active (non-identity) point colours.
    pub fn compile(points: &[PointColour]) -> Self {
        let active: Vec<CompiledPoint> = points
            .iter()
            .filter(|p| !p.is_identity())
            .map(|p| {
                let r = p.range.clamp(0.0, 100.0);
                // Why each constant has its value:
                // - Hue width `8.0 + 0.52 * range`: at range 0, an 8° width provides a narrow slice
                //   to isolate subtle tint differences; at range 50, 34° covers an entire color family;
                //   at range 100, 60° reaches across adjacent primary/secondary sectors.
                // - Chroma width `0.04 + 0.002 * range`: in OkLCh, standard sRGB chroma peaks around
                //   0.35. A base width of 0.04 isolates muted vs vivid tones; expanding to 0.24 at
                //   range 100 spans nearly the entire usable chroma range.
                // - Lightness width `0.12 + 0.004 * range`: in OkLCh, lightness spans [0, 1]. A base
                //   width of 0.12 separates shadows/midtones/highlights; expanding to 0.52 at range 100
                //   encompasses half the entire tonal scale.
                // - `hue_shift * 0.3`: scales the -100..100 slider to a max deflection of ±30°,
                //   giving delicate control over photographic shifts (e.g. skin tones or greenery)
                //   without extreme, unnatural color inversions.
                // - `saturation / 100.0`: scales -100..100 to -1.0..+1.0 so that -100 desaturates
                //   to 0.0 and +100 doubles the chroma.
                // - `(luminance / 100.0) * 0.3`: scales -100..100 to at most ±0.3 OkLCh lightness,
                //   preventing harsh highlight clipping or black crushing.
                CompiledPoint {
                    h0: p.hue.rem_euclid(360.0),
                    c0: p.chroma.max(0.0),
                    l0: p.lightness.clamp(0.0, 1.0),
                    w_h: 8.0 + 0.52 * r,
                    w_c: 0.04 + 0.002 * r,
                    w_l: 0.12 + 0.004 * r,
                    hue_shift_scaled: p.hue_shift.clamp(-100.0, 100.0) * 0.3,
                    sat_scaled: p.saturation.clamp(-100.0, 100.0) / 100.0,
                    lum_scaled: (p.luminance.clamp(-100.0, 100.0) / 100.0) * 0.3,
                }
            })
            .collect();

        Self { points: active }
    }

    /// Empty identity compiled point colours.
    pub fn identity() -> Self {
        Self { points: Vec::new() }
    }

    /// True if no active point colours are present.
    pub fn is_identity(&self) -> bool {
        self.points.is_empty()
    }

    /// Evaluates point colour adjustments on an Oklab pixel (l, a, b) -> (l', a', b').
    #[inline(always)]
    pub fn apply(&self, l: f32, a: f32, b: f32) -> (f32, f32, f32) {
        if self.is_identity() {
            return (l, a, b);
        }

        let c_sq = a * a + b * b;
        const C_FADE_MIN_SQ: f32 =
            crate::media::edit::hsl::CHROMA_FADE_MIN * crate::media::edit::hsl::CHROMA_FADE_MIN;
        if c_sq <= C_FADE_MIN_SQ {
            return (l, a, b);
        }

        let c = c_sq.sqrt();
        let fade = crate::media::edit::hsl::chroma_fade(c);
        if fade <= 0.0 {
            return (l, a, b);
        }

        let h = b.atan2(a).to_degrees().rem_euclid(360.0);

        let mut delta_h = 0.0f32;
        let mut delta_c = 0.0f32;
        let mut delta_l = 0.0f32;

        for pt in &self.points {
            let mut d_h = (h - pt.h0).abs();
            if d_h > 180.0 {
                d_h = 360.0 - d_h;
            }
            if d_h >= pt.w_h {
                continue;
            }

            let d_c = (c - pt.c0).abs();
            if d_c >= pt.w_c {
                continue;
            }

            let d_l = (l - pt.l0).abs();
            if d_l >= pt.w_l {
                continue;
            }

            // Each weight is 1 - smoothstep(0.5, 1.0, distance / width).
            // Inner half (distance <= 0.5 * width) gets full weight 1.0;
            // outer half smoothly falls off to 0.0 using cubic Hermite interpolation.
            let w_h = 1.0 - smoothstep(0.5, 1.0, d_h / pt.w_h);
            let w_c = 1.0 - smoothstep(0.5, 1.0, d_c / pt.w_c);
            let w_l = 1.0 - smoothstep(0.5, 1.0, d_l / pt.w_l);

            let w = w_h * w_c * w_l * fade;
            if w <= 0.0 {
                continue;
            }

            delta_h += w * pt.hue_shift_scaled;
            delta_c += w * pt.sat_scaled;
            delta_l += w * pt.lum_scaled;
        }

        if delta_h == 0.0 && delta_c == 0.0 && delta_l == 0.0 {
            return (l, a, b);
        }

        let final_h = (h + delta_h).rem_euclid(360.0);
        let final_c = (c * (1.0 + delta_c).max(0.0)).max(0.0);
        let final_l = (l + delta_l).clamp(0.0, 1.0);

        let (sin_h, cos_h) = final_h.to_radians().sin_cos();
        (final_l, final_c * cos_h, final_c * sin_h)
    }
}
