//! Local adjustments: masks and the adjustments each one carries (ED-19).
//!
//! ### Where a mask lives
//! Positions are normalised to the **stored** frame — the pixel grid of the file as written,
//! before EXIF orientation, rotation, straighten, flip or crop — with `u` across its width
//! and `v` down its height, both on [0, 1]. Every output pixel is mapped back there through
//! the same affine the geometry samples pixels with (`GeometryPlan::normalised_affine`), and
//! the mask is evaluated at that point. So a crop, a straighten or a rotation can never slide
//! a mask off what it was drawn on, and a gradient is computed exactly at every resolution
//! rather than rasterised and resampled.
//!
//! Shapes are measured in the stored frame's **pixel** proportions (normalised by its long
//! edge), so a radial mask drawn as a circle stays a circle on a 3:2 frame.
//!
//! ### How a mask adjusts
//! Each pixel's effective adjustments are the global ones plus Σ (weight × the mask's own).
//! The frame is rendered once, whatever the number of masks; rendering it once per mask and
//! blending would multiply the cost by the mask count. The locally adjustable set is the one
//! the per-pixel pass can evaluate without a precomputed table: exposure, white balance, the
//! four tone regions, contrast, saturation and vibrance. Curves, HSL, grading, LUTs, effects
//! and looks stay global.
//!
//! ### What a mask is not
//! A mask belongs to its photograph. Presets (`tools::presets`) and batches
//! (`tools::bulk_edit`) never carry masks, for the same reason they never carry a crop.

use super::brush::{BrushRaster, Stroke};
use super::geometry::Geometry;
use super::pipeline::LinearBuffer;
use super::raster::{CoverageRaster, StoredRaster};
use phototools_core::error::Error;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// The adjustments a mask applies where it covers, in the units of the global sliders.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LocalAdjustments {
    /// EV stops.
    pub exposure: f32,
    pub contrast: f32,
    pub highlights: f32,
    pub shadows: f32,
    pub whites: f32,
    pub blacks: f32,
    pub temperature: f32,
    pub tint: f32,
    pub saturation: f32,
    pub vibrance: f32,
}

impl LocalAdjustments {
    pub fn is_zero(&self) -> bool {
        *self == Self::default()
    }

    #[inline(always)]
    fn add_scaled(&mut self, other: &Self, w: f32) {
        self.exposure += w * other.exposure;
        self.contrast += w * other.contrast;
        self.highlights += w * other.highlights;
        self.shadows += w * other.shadows;
        self.whites += w * other.whites;
        self.blacks += w * other.blacks;
        self.temperature += w * other.temperature;
        self.tint += w * other.tint;
        self.saturation += w * other.saturation;
        self.vibrance += w * other.vibrance;
    }
}

/// Local detail adjustments evaluated per mask (RP-11b).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LocalDetail {
    /// Clarity: -100..100, default 0.
    pub clarity: f32,
    /// Texture: -100..100, default 0.
    pub texture: f32,
    /// Dehaze: -100..100, default 0.
    pub dehaze: f32,
    /// Sharpness: 0..100, default 0.
    pub sharpness: f32,
}

impl LocalDetail {
    pub fn is_zero(&self) -> bool {
        self.clarity == 0.0 && self.texture == 0.0 && self.dehaze == 0.0 && self.sharpness == 0.0
    }
}

/// The shape of a mask, in stored-frame coordinates (see the module documentation).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MaskKind {
    /// Full effect on the `start` side of the line through `start`, none beyond the line
    /// through `end`, both perpendicular to `start → end`, with a smooth ramp between: a
    /// graduated filter.
    Linear { start: [f32; 2], end: [f32; 2] },
    /// Full effect inside the inner ellipse, none outside the outer one. `radius_x` and
    /// `radius_y` are fractions of the stored long edge; `angle` turns the ellipse
    /// clockwise, in degrees; `feather` is the fraction of the radius the effect fades over.
    Radial {
        center: [f32; 2],
        radius_x: f32,
        radius_y: f32,
        #[serde(default)]
        angle: f32,
        #[serde(default = "default_feather")]
        feather: f32,
    },
    /// No shape of its own: covered only where painted (ED-21).
    Brush,
    /// Made by a model from the photograph's own pixels and stored with it (`raster`, ED-23).
    Auto {
        target: AutoTarget,
        mask: StoredRaster,
    },
    /// Luminance range: 0..1 on sqrt of linear Rec.709 luminance (RP-11).
    LuminanceRange {
        #[serde(default)]
        low: f32,
        #[serde(default = "default_high")]
        high: f32,
        #[serde(default = "default_feather")]
        feather: f32,
    },
    /// Colour range: OkLCh target and range tolerance matching point colour adjustments (RP-11).
    ColourRange {
        #[serde(default)]
        hue: f32,
        #[serde(default)]
        chroma: f32,
        #[serde(default)]
        lightness: f32,
        #[serde(default = "default_range")]
        range: f32,
    },
}

/// Operation for combining mask components (RP-11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaskOp {
    Add,
    Subtract,
    Intersect,
}

/// A component shape combined with a mask's base shape (RP-11).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaskComponent {
    pub op: MaskOp,
    pub kind: MaskKind,
    #[serde(default)]
    pub invert: bool,
}

/// What an automatic mask finds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoTarget {
    Subject,
    Sky,
}

/// Where compiling a mask gets its pixels: the brush rasters of a mask's strokes and the
/// decoded raster of an automatic mask. A preview session caches both so a frame does not
/// redraw a stroke or decode a PNG it has already seen; an export computes them once.
pub trait MaskRasters {
    fn brush(&mut self, m: &Mask) -> Arc<BrushRaster>;
    fn stored(
        &mut self,
        m: &Mask,
        comp_idx: Option<usize>,
        raster: &StoredRaster,
    ) -> Result<Arc<CoverageRaster>, Error>;
}

/// Computes everything afresh: what an export, which renders once, needs.
pub struct Uncached {
    pub stored_w: u32,
    pub stored_h: u32,
}

impl MaskRasters for Uncached {
    fn brush(&mut self, m: &Mask) -> Arc<BrushRaster> {
        super::brush::rasterise(&m.strokes, self.stored_w, self.stored_h)
    }
    fn stored(
        &mut self,
        _m: &Mask,
        _comp_idx: Option<usize>,
        raster: &StoredRaster,
    ) -> Result<Arc<CoverageRaster>, Error> {
        Ok(Arc::new(raster.decode()?))
    }
}

fn default_feather() -> f32 {
    0.5
}

fn default_high() -> f32 {
    1.0
}

fn default_range() -> f32 {
    50.0
}

fn default_opacity() -> f32 {
    1.0
}

fn default_enabled() -> bool {
    true
}

/// One mask and the adjustments it carries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mask {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub kind: MaskKind,
    /// Apply where the shape is *not*: the outside of a radial, the far side of a gradient.
    #[serde(default)]
    pub invert: bool,
    /// Scales the whole effect, [0, 1].
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    /// A disabled mask is kept with its settings but has no effect.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub adjustments: LocalAdjustments,
    /// Local detail adjustments: clarity, texture, dehaze and sharpness (RP-11b).
    #[serde(default, skip_serializing_if = "LocalDetail::is_zero")]
    pub detail: LocalDetail,
    /// Brush strokes refining the shape, in order, after `invert` (`brush`, ED-21): an
    /// added stroke always adds effect where it is painted, whatever the shape or invert.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub strokes: Vec<Stroke>,
    /// Components modifying this mask's coverage (RP-11).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub components: Vec<MaskComponent>,
}

impl Mask {
    /// True when the mask cannot change any pixel.
    pub fn is_identity(&self) -> bool {
        !self.enabled
            || self.opacity <= 0.0
            || (self.adjustments.is_zero() && self.detail.is_zero())
    }
}

/// Smooth 0 → 1 over [0, 1], clamped: C¹ at both ends, so a ramp shows no edge where it
/// starts or stops.
#[inline(always)]
fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[derive(Debug, Clone)]
enum Shape {
    /// Origin and direction scaled so `t = (p − start)·dir` runs 0 → 1 between the lines.
    Linear { sx: f32, sy: f32, dx: f32, dy: f32 },
    Radial {
        cx: f32,
        cy: f32,
        cos: f32,
        sin: f32,
        inv_rx: f32,
        inv_ry: f32,
        inner: f32,
        inv_feather: f32,
    },
    /// A degenerate shape (a gradient with no length), or a brush mask's: covers nothing.
    Empty,
    /// An automatic mask's stored pixels, sampled in stored-normalised coordinates.
    Raster(Arc<CoverageRaster>),
    /// Luminance range: 0..1 on sqrt of linear Rec.709 luminance (RP-11).
    LuminanceRange { low: f32, high: f32, feather: f32 },
    /// Colour range matching OkLCh point colour adjustments (RP-11).
    ColourRange {
        h0: f32,
        c0: f32,
        l0: f32,
        w_h: f32,
        w_c: f32,
        w_l: f32,
    },
}

impl Shape {
    fn is_luminance(&self) -> bool {
        matches!(self, Shape::LuminanceRange { .. })
    }

    fn is_colour(&self) -> bool {
        matches!(self, Shape::ColourRange { .. })
    }

    #[inline(always)]
    fn coverage(
        &self,
        x: f32,
        y: f32,
        ax: f32,
        ay: f32,
        y_sqrt: f32,
        oklch: (f32, f32, f32),
    ) -> f32 {
        match *self {
            Shape::Linear { sx, sy, dx, dy } => {
                let t = (x - sx) * dx + (y - sy) * dy;
                1.0 - smoothstep(t)
            }
            Shape::Radial {
                cx,
                cy,
                cos,
                sin,
                inv_rx,
                inv_ry,
                inner,
                inv_feather,
            } => {
                let (qx, qy) = (x - cx, y - cy);
                // Into the ellipse's own axes: rotate the offset back by the ellipse's angle.
                let ex = (qx * cos + qy * sin) * inv_rx;
                let ey = (-qx * sin + qy * cos) * inv_ry;
                let r2 = ex * ex + ey * ey;
                if r2 >= 1.0 {
                    0.0
                } else {
                    let r = r2.sqrt();
                    if r <= inner {
                        1.0
                    } else {
                        1.0 - smoothstep((r - inner) * inv_feather)
                    }
                }
            }
            Shape::Empty => 0.0,
            Shape::Raster(ref r) => r.sample(x / ax, y / ay),
            Shape::LuminanceRange { low, high, feather } => {
                let (low, high) = if low <= high {
                    (low, high)
                } else {
                    (high, low)
                };
                if feather <= 1e-6 {
                    if y_sqrt >= low && y_sqrt <= high {
                        1.0
                    } else {
                        0.0
                    }
                } else {
                    let dist = if y_sqrt < low {
                        low - y_sqrt
                    } else if y_sqrt > high {
                        y_sqrt - high
                    } else {
                        0.0
                    };
                    if dist <= 0.0 {
                        1.0
                    } else if dist >= feather {
                        0.0
                    } else {
                        1.0 - smoothstep(dist / feather)
                    }
                }
            }
            Shape::ColourRange {
                h0,
                c0,
                l0,
                w_h,
                w_c,
                w_l,
            } => crate::media::edit::point_colour::colour_weight(
                h0, c0, l0, w_h, w_c, w_l, oklch.0, oklch.1, oklch.2,
            ),
        }
    }
}

#[derive(Debug, Clone)]
struct CompiledComponent {
    op: MaskOp,
    shape: Shape,
    invert: bool,
}

#[derive(Debug, Clone)]
pub struct CompiledMask {
    pub id: String,
    pub detail: LocalDetail,
    shape: Shape,
    components: Vec<CompiledComponent>,
    invert: bool,
    opacity: f32,
    adjustments: LocalAdjustments,
    brush: Option<Arc<BrushRaster>>,
    /// Stored width and height over the long edge, to recover stored-normalised
    /// coordinates for the brush rasters from the aspect-scaled ones shapes use.
    ax: f32,
    ay: f32,
}

impl CompiledMask {
    fn needs_luminance(&self) -> bool {
        self.shape.is_luminance() || self.components.iter().any(|c| c.shape.is_luminance())
    }

    fn needs_colour(&self) -> bool {
        self.shape.is_colour() || self.components.iter().any(|c| c.shape.is_colour())
    }

    #[inline(always)]
    fn coverage(&self, x: f32, y: f32, y_sqrt: f32, oklch: (f32, f32, f32)) -> f32 {
        let mut c = self.shape.coverage(x, y, self.ax, self.ay, y_sqrt, oklch);
        if self.invert {
            c = 1.0 - c;
        }
        for comp in &self.components {
            let mut ci = comp.shape.coverage(x, y, self.ax, self.ay, y_sqrt, oklch);
            if comp.invert {
                ci = 1.0 - ci;
            }
            match comp.op {
                MaskOp::Add => c = c.max(ci),
                MaskOp::Subtract => c *= 1.0 - ci,
                MaskOp::Intersect => c *= ci,
            }
        }
        c
    }

    #[inline(always)]
    pub fn weight(&self, x: f32, y: f32, y_sqrt: f32, oklch: (f32, f32, f32)) -> f32 {
        let c = self.coverage(x, y, y_sqrt, oklch);
        let c = match &self.brush {
            Some(b) => b.apply(c, x / self.ax, y / self.ay),
            None => c,
        };
        c * self.opacity
    }
}

/// The masks of one recipe, resolved against one rendering: ready to be evaluated per pixel.
#[derive(Debug, Clone)]
pub struct CompiledMasks {
    /// Output normalised → stored normalised (`GeometryPlan::normalised_affine`).
    affine: [f32; 6],
    /// Stored width and height over the stored long edge.
    pub ax: f32,
    pub ay: f32,
    masks: Vec<CompiledMask>,
    /// Which groups of adjustments any mask touches, so the pixel pass enables a stage
    /// that is at rest globally but active locally.
    pub touches_tone: bool,
    pub touches_contrast: bool,
    pub touches_saturation: bool,
    pub has_range_masks: bool,
    pub needs_luminance: bool,
    pub needs_colour: bool,
}

impl CompiledMasks {
    /// Resolves `masks` for a rendering of a stored frame of `stored_w` × `stored_h` in EXIF
    /// `orientation`, through `geometry` when the rendering applies one.
    ///
    /// `None` when no mask can change a pixel, so a photograph without active masks takes
    /// exactly the path it took before masks existed.
    pub fn compile(
        masks: &[Mask],
        geometry: Option<&Geometry>,
        stored_w: u32,
        stored_h: u32,
        orientation: u32,
    ) -> Result<Option<Self>, Error> {
        Self::compile_with(
            masks,
            geometry,
            stored_w,
            stored_h,
            orientation,
            &mut Uncached { stored_w, stored_h },
        )
    }

    /// As `compile`, with brush and stored rasters from `rasters` — a preview session's
    /// cache, so painting redraws only what changed and a stored mask is decoded once.
    pub fn compile_with(
        masks: &[Mask],
        geometry: Option<&Geometry>,
        stored_w: u32,
        stored_h: u32,
        orientation: u32,
        rasters: &mut dyn MaskRasters,
    ) -> Result<Option<Self>, Error> {
        let active: Vec<&Mask> = masks.iter().filter(|m| !m.is_identity()).collect();
        if active.is_empty() {
            return Ok(None);
        }

        let affine = output_to_stored(geometry, stored_w, stored_h, orientation)?;
        let (ax, ay) = aspect(stored_w, stored_h);

        let compiled = active
            .iter()
            .map(|m| compile_mask(m, ax, ay, rasters))
            .collect::<Result<Vec<_>, Error>>()?;

        let any = |f: fn(&LocalAdjustments) -> bool| compiled.iter().any(|m| f(&m.adjustments));
        let needs_luminance = compiled.iter().any(|m| m.needs_luminance());
        let needs_colour = compiled.iter().any(|m| m.needs_colour());
        let has_range_masks = needs_luminance || needs_colour;

        Ok(Some(Self {
            affine,
            ax,
            ay,
            touches_tone: any(|a| {
                a.highlights != 0.0 || a.shadows != 0.0 || a.whites != 0.0 || a.blacks != 0.0
            }),
            touches_contrast: any(|a| a.contrast != 0.0),
            touches_saturation: any(|a| a.saturation != 0.0 || a.vibrance != 0.0),
            has_range_masks,
            needs_luminance,
            needs_colour,
            masks: compiled,
        }))
    }

    /// True if any active mask carries non-zero local detail (RP-11b).
    pub fn has_local_detail(&self) -> bool {
        self.masks.iter().any(|m| !m.detail.is_zero())
    }

    /// The active compiled masks.
    pub fn active_masks(&self) -> &[CompiledMask] {
        &self.masks
    }

    /// Where a row of `out_w` output pixels at output row `y` starts in aspect-scaled stored
    /// coordinates, and how far one pixel to the right moves it. The map is affine, so the
    /// pixel loop advances by addition rather than evaluating the map per pixel.
    #[inline]
    pub fn row(&self, y: u32, out_w: u32, out_h: u32) -> RowCursor {
        let [a, b, c, d, e, f] = self.affine;
        let u0 = 0.5 / out_w as f32;
        let v = (y as f32 + 0.5) / out_h as f32;
        let du = 1.0 / out_w as f32;
        RowCursor {
            x: (a * u0 + b * v + c) * self.ax,
            y: (d * u0 + e * v + f) * self.ay,
            dx: a * du * self.ax,
            dy: d * du * self.ay,
        }
    }

    /// The summed local adjustments at a point given by a `RowCursor`, reading incoming linear RGB (RP-11).
    #[inline(always)]
    pub fn local_at(&self, x: f32, y: f32, r: f32, g: f32, b: f32) -> LocalAdjustments {
        let (y_sqrt, oklch) = if self.has_range_masks {
            let y_sqrt = if self.needs_luminance {
                (0.2126 * r + 0.7152 * g + 0.0722 * b).max(0.0).sqrt()
            } else {
                0.0
            };
            let oklch = if self.needs_colour {
                let (l, a, b_val) = crate::media::edit::color::linear_srgb_to_oklab(r, g, b);
                let c = (a * a + b_val * b_val).sqrt();
                let h = b_val.atan2(a).to_degrees().rem_euclid(360.0);
                (h, c, l)
            } else {
                (0.0, 0.0, 0.0)
            };
            (y_sqrt, oklch)
        } else {
            (0.0, (0.0, 0.0, 0.0))
        };

        let mut sum = LocalAdjustments::default();
        for m in &self.masks {
            let w = m.weight(x, y, y_sqrt, oklch);
            if w > 0.0 {
                sum.add_scaled(&m.adjustments, w);
            }
        }
        sum
    }
}

/// The map from a rendering's output to the stored frame, both normalised on [0, 1]:
/// `u_s = a·u + b·v + c`, `v_s = d·u + e·v + f` for `[a, b, c, d, e, f]`.
///
/// Without geometry the output is the stored frame, pixel for pixel (ED-13 only bakes the
/// orientation in when a geometry is present). The Edit view receives this with every
/// frame, so the handles it draws sit where the masks act without the view knowing any of
/// the geometry's rules.
pub fn output_to_stored(
    geometry: Option<&Geometry>,
    stored_w: u32,
    stored_h: u32,
    orientation: u32,
) -> Result<[f32; 6], Error> {
    Ok(match geometry {
        Some(g) => g.plan(stored_w, stored_h, orientation)?.normalised_affine(),
        None => [1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
    })
}

/// Stored width and height over the stored long edge: the units shapes are measured in.
fn aspect(stored_w: u32, stored_h: u32) -> (f32, f32) {
    let long = stored_w.max(stored_h).max(1) as f32;
    (stored_w as f32 / long, stored_h as f32 / long)
}

/// Where one mask acts, as 8-bit coverage over an `out_w` × `out_h` rendering, for the
/// overlay that shows a mask while it is shaped (ED-20).
///
/// Includes invert and opacity but not whether the mask has adjustments yet: a new mask
/// is shown before it does anything. Computed here, by the same formula the pixels use,
/// so the overlay cannot disagree with the effect.
#[allow(clippy::too_many_arguments)]
pub fn coverage_frame(
    mask: &Mask,
    rasters: &mut dyn MaskRasters,
    affine: [f32; 6],
    stored_w: u32,
    stored_h: u32,
    out_w: u32,
    out_h: u32,
    pixels: &[f32],
) -> Result<Vec<u8>, Error> {
    use rayon::prelude::*;
    let (ax, ay) = aspect(stored_w, stored_h);
    let compiled = compile_mask(mask, ax, ay, rasters)?;
    let needs_luminance = compiled.needs_luminance();
    let needs_colour = compiled.needs_colour();
    let has_range = needs_luminance || needs_colour;
    let one = CompiledMasks {
        affine,
        ax,
        ay,
        touches_tone: false,
        touches_contrast: false,
        touches_saturation: false,
        has_range_masks: has_range,
        needs_luminance,
        needs_colour,
        masks: vec![compiled],
    };
    let mut out = vec![0u8; (out_w as usize) * (out_h as usize)];
    out.par_chunks_exact_mut(out_w.max(1) as usize)
        .enumerate()
        .for_each(|(y, row)| {
            let cursor = one.row(y as u32, out_w, out_h);
            let px_row_start = y * (out_w as usize) * 3;
            for (x, px) in row.iter_mut().enumerate() {
                let (mx, my) = cursor.at(x);
                let (y_sqrt, oklch) = if has_range && px_row_start + x * 3 + 2 < pixels.len() {
                    let r = pixels[px_row_start + x * 3];
                    let g = pixels[px_row_start + x * 3 + 1];
                    let b = pixels[px_row_start + x * 3 + 2];
                    let y_sqrt = if needs_luminance {
                        (0.2126 * r + 0.7152 * g + 0.0722 * b).max(0.0).sqrt()
                    } else {
                        0.0
                    };
                    let oklch = if needs_colour {
                        let (l, a, b_val) =
                            crate::media::edit::color::linear_srgb_to_oklab(r, g, b);
                        let c = (a * a + b_val * b_val).sqrt();
                        let h = b_val.atan2(a).to_degrees().rem_euclid(360.0);
                        (h, c, l)
                    } else {
                        (0.0, 0.0, 0.0)
                    };
                    (y_sqrt, oklch)
                } else {
                    (0.0, (0.0, 0.0, 0.0))
                };
                let w = one.masks[0].weight(mx, my, y_sqrt, oklch);
                // Any weight at all shows: a weight that rounds to 0 can still move a pixel
                // by a code value, and the overlay must not hide where the mask acts.
                *px = if w > 0.0 {
                    ((w * 255.0 + 0.5) as u8).max(1)
                } else {
                    0
                };
            }
        });
    Ok(out)
}

/// Blends local detail frames into `base` linear buffer according to mask coverage (RP-11b).
/// With `B` the base frame and `w` a mask's full weight at that pixel,
/// the composite is `B + Σ w × (D − B)` over the masks with local detail.
/// Selection of range masks always reads `base` (`B`), never `D` or the composite.
pub fn blend_local_detail(
    base: &LinearBuffer,
    detail_frames: &[(&CompiledMask, &LinearBuffer)],
    masks: &CompiledMasks,
) -> LinearBuffer {
    if detail_frames.is_empty() {
        return base.clone();
    }

    let w = base.width as usize;
    let mut out = base.clone();

    out.data
        .par_chunks_exact_mut(w * 3)
        .enumerate()
        .for_each(|(y, row_out)| {
            let cursor = masks.row(y as u32, base.width, base.height);
            let row_start = y * w * 3;
            let base_row = &base.data[row_start..row_start + w * 3];

            for x in 0..w {
                let px_idx = x * 3;
                let (mx, my) = cursor.at(x);

                let r_b = base_row[px_idx];
                let g_b = base_row[px_idx + 1];
                let b_b = base_row[px_idx + 2];

                let (y_sqrt, oklch) = if masks.has_range_masks {
                    let y_sqrt = if masks.needs_luminance {
                        (0.2126 * r_b + 0.7152 * g_b + 0.0722 * b_b).max(0.0).sqrt()
                    } else {
                        0.0
                    };
                    let oklch = if masks.needs_colour {
                        let (l, a, b_val) =
                            crate::media::edit::color::linear_srgb_to_oklab(r_b, g_b, b_b);
                        let c = (a * a + b_val * b_val).sqrt();
                        let h = b_val.atan2(a).to_degrees().rem_euclid(360.0);
                        (h, c, l)
                    } else {
                        (0.0, 0.0, 0.0)
                    };
                    (y_sqrt, oklch)
                } else {
                    (0.0, (0.0, 0.0, 0.0))
                };

                let mut sum_r = 0.0f32;
                let mut sum_g = 0.0f32;
                let mut sum_b = 0.0f32;

                for (compiled_mask, d_buf) in detail_frames {
                    let weight = compiled_mask.weight(mx, my, y_sqrt, oklch);
                    if weight > 0.0 {
                        let r_d = d_buf.data[row_start + px_idx];
                        let g_d = d_buf.data[row_start + px_idx + 1];
                        let b_d = d_buf.data[row_start + px_idx + 2];
                        sum_r += weight * (r_d - r_b);
                        sum_g += weight * (g_d - g_b);
                        sum_b += weight * (b_d - b_b);
                    }
                }

                row_out[px_idx] = r_b + sum_r;
                row_out[px_idx + 1] = g_b + sum_g;
                row_out[px_idx + 2] = b_b + sum_b;
            }
        });

    out
}

/// A position along one output row, in aspect-scaled stored coordinates.
#[derive(Debug, Clone, Copy)]
pub struct RowCursor {
    pub x: f32,
    pub y: f32,
    pub dx: f32,
    pub dy: f32,
}

impl RowCursor {
    /// The point `i` pixels from the start of the row.
    #[inline(always)]
    pub fn at(&self, i: usize) -> (f32, f32) {
        let i = i as f32;
        (self.x + i * self.dx, self.y + i * self.dy)
    }
}

fn compile_mask(
    m: &Mask,
    ax: f32,
    ay: f32,
    rasters: &mut dyn MaskRasters,
) -> Result<CompiledMask, Error> {
    let shape = match &m.kind {
        MaskKind::Auto { mask, .. } => Shape::Raster(rasters.stored(m, None, mask)?),
        kind => compile_shape(kind, ax, ay),
    };
    let mut compiled_components = Vec::with_capacity(m.components.len());
    for (i, comp) in m.components.iter().enumerate() {
        let shape = match &comp.kind {
            MaskKind::Auto { mask, .. } => Shape::Raster(rasters.stored(m, Some(i), mask)?),
            kind => compile_shape(kind, ax, ay),
        };
        compiled_components.push(CompiledComponent {
            op: comp.op,
            shape,
            invert: comp.invert,
        });
    }
    Ok(CompiledMask {
        id: m.id.clone(),
        detail: m.detail,
        shape,
        components: compiled_components,
        invert: m.invert,
        opacity: m.opacity.clamp(0.0, 1.0),
        adjustments: m.adjustments,
        brush: if m.strokes.is_empty() {
            None
        } else {
            Some(rasters.brush(m))
        },
        ax,
        ay,
    })
}

fn compile_shape(kind: &MaskKind, ax: f32, ay: f32) -> Shape {
    match *kind {
        // Resolved in `compile_mask`, which can reach the decoded pixels.
        MaskKind::Brush | MaskKind::Auto { .. } => Shape::Empty,
        MaskKind::Linear { start, end } => {
            let (sx, sy) = (start[0] * ax, start[1] * ay);
            let (vx, vy) = ((end[0] - start[0]) * ax, (end[1] - start[1]) * ay);
            let len2 = vx * vx + vy * vy;
            if len2 < 1e-12 {
                Shape::Empty
            } else {
                Shape::Linear {
                    sx,
                    sy,
                    dx: vx / len2,
                    dy: vy / len2,
                }
            }
        }
        MaskKind::Radial {
            center,
            radius_x,
            radius_y,
            angle,
            feather,
        } => {
            let feather = feather.clamp(0.0, 1.0);
            let (sin, cos) = angle.to_radians().sin_cos();
            Shape::Radial {
                cx: center[0] * ax,
                cy: center[1] * ay,
                cos,
                sin,
                inv_rx: 1.0 / radius_x.max(1e-4),
                inv_ry: 1.0 / radius_y.max(1e-4),
                inner: 1.0 - feather,
                // A hard edge when feather is 0: anything past the radius is outside.
                inv_feather: 1.0 / feather.max(1e-6),
            }
        }
        MaskKind::LuminanceRange { low, high, feather } => Shape::LuminanceRange {
            low,
            high,
            feather: feather.max(0.0),
        },
        MaskKind::ColourRange {
            hue,
            chroma,
            lightness,
            range,
        } => {
            let (w_h, w_c, w_l) = crate::media::edit::point_colour::point_colour_widths(range);
            Shape::ColourRange {
                h0: hue.rem_euclid(360.0),
                c0: chroma.max(0.0),
                l0: lightness.clamp(0.0, 1.0),
                w_h,
                w_c,
                w_l,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mask(kind: MaskKind, exposure: f32) -> Mask {
        Mask {
            id: "m".into(),
            name: String::new(),
            kind,
            invert: false,
            opacity: 1.0,
            enabled: true,
            adjustments: LocalAdjustments {
                exposure,
                ..Default::default()
            },
            strokes: Vec::new(),
            components: Vec::new(),
            detail: LocalDetail::default(),
        }
    }

    fn compiled(m: Mask, w: u32, h: u32) -> CompiledMasks {
        CompiledMasks::compile(&[m], None, w, h, 1)
            .unwrap()
            .unwrap()
    }

    /// Exposure the mask contributes at stored-normalised `(u, v)` of a `w` × `h` frame.
    fn exposure_at(c: &CompiledMasks, u: f32, v: f32) -> f32 {
        c.local_at(u * c.ax, v * c.ay, 0.5, 0.5, 0.5).exposure
    }

    #[test]
    fn a_linear_gradient_ramps_monotonically_between_its_lines() {
        let c = compiled(
            mask(
                MaskKind::Linear {
                    start: [0.5, 0.2],
                    end: [0.5, 0.8],
                },
                1.0,
            ),
            300,
            200,
        );
        assert_eq!(
            exposure_at(&c, 0.3, 0.1),
            1.0,
            "full effect before the start line"
        );
        assert_eq!(
            exposure_at(&c, 0.9, 0.9),
            0.0,
            "no effect beyond the end line"
        );
        assert!(
            (exposure_at(&c, 0.1, 0.5) - 0.5).abs() < 1e-5,
            "half way at the middle"
        );

        let mut last = f32::INFINITY;
        for i in 0..=100 {
            let e = exposure_at(&c, 0.5, i as f32 / 100.0);
            assert!(
                e <= last + 1e-6,
                "not monotonic at v = {}",
                i as f32 / 100.0
            );
            last = e;
        }
    }

    #[test]
    fn a_radial_mask_is_symmetric_and_feathers_smoothly() {
        let c = compiled(
            mask(
                MaskKind::Radial {
                    center: [0.5, 0.5],
                    radius_x: 0.2,
                    radius_y: 0.2,
                    angle: 0.0,
                    feather: 0.5,
                },
                1.0,
            ),
            300,
            200,
        );
        assert_eq!(exposure_at(&c, 0.5, 0.5), 1.0);
        // 0.2 of the long edge (300 px) is 60 px: 0.2 of the width, 0.3 of the height.
        assert_eq!(exposure_at(&c, 0.5 + 0.21, 0.5), 0.0);
        assert_eq!(exposure_at(&c, 0.5, 0.5 + 0.31), 0.0);

        // A circle in pixels: the same falloff along the width and the height.
        for k in 0..=20 {
            let r = k as f32 / 20.0 * 0.2; // fraction of the long edge
            let along_x = exposure_at(&c, 0.5 + r, 0.5);
            let along_y = exposure_at(&c, 0.5, 0.5 + r * 1.5);
            let mirror = exposure_at(&c, 0.5 - r, 0.5);
            assert!((along_x - along_y).abs() < 1e-4, "not circular at r = {r}");
            assert!((along_x - mirror).abs() < 1e-5, "not symmetric at r = {r}");
        }

        // Smooth: no step larger than a fine ramp allows anywhere across the feather.
        let mut last = 1.0;
        for i in 0..=400 {
            let e = exposure_at(&c, 0.5 + 0.2 * i as f32 / 400.0, 0.5);
            assert!((last - e).abs() < 0.02, "a step at {i}");
            last = e;
        }
    }

    #[test]
    fn inverting_a_mask_complements_its_coverage() {
        let kind = MaskKind::Radial {
            center: [0.4, 0.6],
            radius_x: 0.15,
            radius_y: 0.1,
            angle: 30.0,
            feather: 0.6,
        };
        let plain = compiled(mask(kind.clone(), 1.0), 400, 300);
        let mut inv = mask(kind, 1.0);
        inv.invert = true;
        let inverted = compiled(inv, 400, 300);
        for i in 0..50 {
            for j in 0..50 {
                let (u, v) = (i as f32 / 49.0, j as f32 / 49.0);
                let sum = exposure_at(&plain, u, v) + exposure_at(&inverted, u, v);
                assert!((sum - 1.0).abs() < 1e-5, "at ({u}, {v})");
            }
        }
    }

    #[test]
    fn opacity_scales_and_disabling_removes_a_mask() {
        let kind = MaskKind::Linear {
            start: [0.0, 0.0],
            end: [0.0, 1.0],
        };
        let mut half = mask(kind.clone(), 2.0);
        half.opacity = 0.5;
        assert_eq!(exposure_at(&compiled(half, 10, 10), 0.5, 0.0), 1.0);

        let mut off = mask(kind, 2.0);
        off.enabled = false;
        assert!(CompiledMasks::compile(&[off], None, 10, 10, 1)
            .unwrap()
            .is_none());
    }

    #[test]
    fn a_mask_with_no_adjustments_compiles_to_nothing() {
        let m = mask(
            MaskKind::Linear {
                start: [0.0, 0.0],
                end: [1.0, 1.0],
            },
            0.0,
        );
        assert!(CompiledMasks::compile(&[m], None, 10, 10, 1)
            .unwrap()
            .is_none());
    }

    #[test]
    fn overlapping_masks_add() {
        let everywhere = |e| {
            mask(
                MaskKind::Linear {
                    start: [0.0, 2.0],
                    end: [0.0, 3.0],
                },
                e,
            )
        };
        let c = CompiledMasks::compile(&[everywhere(0.5), everywhere(0.25)], None, 10, 10, 1)
            .unwrap()
            .unwrap();
        assert_eq!(exposure_at(&c, 0.5, 0.5), 0.75);
    }

    #[test]
    fn a_mask_round_trips_through_json_and_old_fields_default() {
        let json = r#"{"id":"a","kind":{"type":"radial","center":[0.5,0.5],"radius_x":0.1,"radius_y":0.2},"adjustments":{"exposure":1.5}}"#;
        let m: Mask = serde_json::from_str(json).unwrap();
        assert!(m.enabled && !m.invert && m.opacity == 1.0);
        assert_eq!(m.adjustments.exposure, 1.5);
        assert!(
            matches!(m.kind, MaskKind::Radial { feather, angle, .. } if feather == 0.5 && angle == 0.0)
        );
        let back: Mask = serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
        assert_eq!(back, m);
    }
}
