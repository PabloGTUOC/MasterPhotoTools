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
use super::raster::{CoverageRaster, StoredRaster};
use crate::error::Error;
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
    fn stored(&mut self, m: &Mask, raster: &StoredRaster) -> Result<Arc<CoverageRaster>, Error>;
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
    fn stored(&mut self, _m: &Mask, raster: &StoredRaster) -> Result<Arc<CoverageRaster>, Error> {
        Ok(Arc::new(raster.decode()?))
    }
}

fn default_feather() -> f32 {
    0.5
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
    /// Brush strokes refining the shape, in order, after `invert` (`brush`, ED-21): an
    /// added stroke always adds effect where it is painted, whatever the shape or invert.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub strokes: Vec<Stroke>,
}

impl Mask {
    /// True when the mask cannot change any pixel.
    pub fn is_identity(&self) -> bool {
        !self.enabled || self.opacity <= 0.0 || self.adjustments.is_zero()
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
}

#[derive(Debug, Clone)]
struct CompiledMask {
    shape: Shape,
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
    /// Coverage at a point in aspect-scaled stored coordinates, before invert and opacity.
    #[inline(always)]
    fn coverage(&self, x: f32, y: f32) -> f32 {
        match self.shape {
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
            Shape::Raster(ref r) => r.sample(x / self.ax, y / self.ay),
        }
    }

    #[inline(always)]
    fn weight(&self, x: f32, y: f32) -> f32 {
        let c = self.coverage(x, y);
        let c = if self.invert { 1.0 - c } else { c };
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
    ax: f32,
    ay: f32,
    masks: Vec<CompiledMask>,
    /// Which groups of adjustments any mask touches, so the pixel pass enables a stage
    /// that is at rest globally but active locally.
    pub touches_tone: bool,
    pub touches_contrast: bool,
    pub touches_saturation: bool,
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
        Ok(Some(Self {
            affine,
            ax,
            ay,
            touches_tone: any(|a| {
                a.highlights != 0.0 || a.shadows != 0.0 || a.whites != 0.0 || a.blacks != 0.0
            }),
            touches_contrast: any(|a| a.contrast != 0.0),
            touches_saturation: any(|a| a.saturation != 0.0 || a.vibrance != 0.0),
            masks: compiled,
        }))
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

    /// The summed local adjustments at a point given by a `RowCursor`.
    #[inline(always)]
    pub fn local_at(&self, x: f32, y: f32) -> LocalAdjustments {
        let mut sum = LocalAdjustments::default();
        for m in &self.masks {
            let w = m.weight(x, y);
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
pub fn coverage_frame(
    mask: &Mask,
    rasters: &mut dyn MaskRasters,
    affine: [f32; 6],
    stored_w: u32,
    stored_h: u32,
    out_w: u32,
    out_h: u32,
) -> Result<Vec<u8>, Error> {
    use rayon::prelude::*;
    let (ax, ay) = aspect(stored_w, stored_h);
    let one = CompiledMasks {
        affine,
        ax,
        ay,
        masks: vec![compile_mask(mask, ax, ay, rasters)?],
        touches_tone: false,
        touches_contrast: false,
        touches_saturation: false,
    };
    let mut out = vec![0u8; (out_w as usize) * (out_h as usize)];
    out.par_chunks_exact_mut(out_w.max(1) as usize)
        .enumerate()
        .for_each(|(y, row)| {
            let cursor = one.row(y as u32, out_w, out_h);
            for (x, px) in row.iter_mut().enumerate() {
                let (mx, my) = cursor.at(x);
                let w = one.masks[0].weight(mx, my);
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
        MaskKind::Auto { mask, .. } => Shape::Raster(rasters.stored(m, mask)?),
        kind => compile_shape(kind, ax, ay),
    };
    Ok(CompiledMask {
        shape,
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
        }
    }

    fn compiled(m: Mask, w: u32, h: u32) -> CompiledMasks {
        CompiledMasks::compile(&[m], None, w, h, 1)
            .unwrap()
            .unwrap()
    }

    /// Exposure the mask contributes at stored-normalised `(u, v)` of a `w` × `h` frame.
    fn exposure_at(c: &CompiledMasks, u: f32, v: f32) -> f32 {
        c.local_at(u * c.ax, v * c.ay).exposure
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
