//! Brush strokes on a mask (ED-21).
//!
//! ### What a stroke does
//! A stroke is a polyline in the stored frame's normalised coordinates with a radius (a
//! fraction of the long edge), a feather and a flow. Its **footprint** `s` at a point is
//! the flow times a smooth falloff of the distance to the polyline — the maximum over its
//! segments, so a stroke that doubles back does not darken itself. Applied to a coverage
//! `c`, an adding stroke gives `c(1 − s) + s` and an erasing one `c(1 − s)`.
//!
//! Both are affine in `c`, so any sequence of strokes is `c ↦ α·c + β` for two values per
//! point. Two rasters therefore hold every stroke a mask has, **in order**: painting over
//! an erased area brings it back, as in any paint program, and a stroke refines whatever
//! the mask's shape gives (a gradient now, an automatic mask later) without knowing it.
//!
//! ### Where it is drawn, and how often
//! The rasters are in the stored frame, like every mask (`masks`), and sampled through the
//! same map from output to stored as the shapes. They are drawn at the stored proxy's size
//! for previews and at most `MAX_RASTER_EDGE` on the long edge for exports.
//!
//! A `BrushCache` keeps the rasters for one mask at one size and, while a stroke is being
//! painted — the last stroke growing by a few points per frame — draws only the new
//! segments. Everything before the last stroke is kept as `α₀, β₀`, and the last stroke's
//! footprint is grown in place, so a frame during painting costs the new segments and one
//! pass to combine, not the whole history.

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// The long edge, in pixels, an export draws strokes at. A feathered stroke drawn at 4096
/// and sampled bilinearly is indistinguishable from one drawn at 36 MP, at a seventh of the
/// memory: 45 MB a raster rather than 150.
pub const MAX_RASTER_EDGE: u32 = 4096;

fn default_feather() -> f32 {
    0.5
}

fn default_flow() -> f32 {
    1.0
}

/// One brush stroke, in stored-frame coordinates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    pub points: Vec<[f32; 2]>,
    /// Fraction of the stored long edge.
    pub radius: f32,
    /// Fraction of the radius over which the stroke fades, [0, 1].
    #[serde(default = "default_feather")]
    pub feather: f32,
    /// Strength, [0, 1].
    #[serde(default = "default_flow")]
    pub flow: f32,
    #[serde(default)]
    pub erase: bool,
}

impl Stroke {
    /// The same stroke with more points: what a stroke being painted looks like from one
    /// frame to the next.
    fn is_extended_by(&self, next: &Stroke) -> bool {
        self.radius == next.radius
            && self.feather == next.feather
            && self.flow == next.flow
            && self.erase == next.erase
            && next.points.len() >= self.points.len()
            && next.points[..self.points.len()] == self.points[..]
    }
}

/// `c ↦ α·c + β` at every point of a stored-frame grid.
#[derive(Debug)]
pub struct BrushRaster {
    pub w: u32,
    pub h: u32,
    alpha: Vec<f32>,
    beta: Vec<f32>,
}

impl BrushRaster {
    /// Applies the strokes to coverage `c` at stored-normalised `(u, v)`, sampling bilinearly.
    #[inline(always)]
    pub fn apply(&self, c: f32, u: f32, v: f32) -> f32 {
        let (w, h) = (self.w as usize, self.h as usize);
        let x = (u * self.w as f32 - 0.5).clamp(0.0, (w - 1) as f32);
        let y = (v * self.h as f32 - 0.5).clamp(0.0, (h - 1) as f32);
        let (x0, y0) = (x as usize, y as usize);
        let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let lerp = |g: &[f32]| {
            let top = g[y0 * w + x0] * (1.0 - fx) + g[y0 * w + x1] * fx;
            let bottom = g[y1 * w + x0] * (1.0 - fx) + g[y1 * w + x1] * fx;
            top * (1.0 - fy) + bottom * fy
        };
        (lerp(&self.alpha) * c + lerp(&self.beta)).clamp(0.0, 1.0)
    }
}

/// The rasters for one mask at one size, kept between frames.
#[derive(Debug)]
pub struct BrushCache {
    w: u32,
    h: u32,
    strokes: Vec<Stroke>,
    /// Every stroke before the last.
    alpha0: Vec<f32>,
    beta0: Vec<f32>,
    /// The last stroke's footprint.
    last: Vec<f32>,
    combined: Arc<BrushRaster>,
    /// Segments drawn since the cache was made, so a test can see what painting costs.
    pub segments_drawn: usize,
}

impl BrushCache {
    pub fn new(w: u32, h: u32) -> Self {
        let n = (w as usize) * (h as usize);
        Self {
            w,
            h,
            strokes: Vec::new(),
            alpha0: vec![1.0; n],
            beta0: vec![0.0; n],
            last: vec![0.0; n],
            combined: Arc::new(BrushRaster {
                w,
                h,
                alpha: vec![1.0; n],
                beta: vec![0.0; n],
            }),
            segments_drawn: 0,
        }
    }

    /// The rasters for `strokes`, drawing only what changed since the last call.
    pub fn update(&mut self, strokes: &[Stroke]) -> Arc<BrushRaster> {
        if strokes == self.strokes.as_slice() {
            return self.combined.clone();
        }
        let (n, old) = (strokes.len(), self.strokes.len());
        // Every stroke before the cached last one is unchanged.
        let prefix_kept = old >= 1 && old <= n && strokes[..old - 1] == self.strokes[..old - 1];

        if prefix_kept && old == n && self.strokes[n - 1].is_extended_by(&strokes[n - 1]) {
            // The last stroke grew: draw its new segments only (from the old last point, so
            // the join is drawn too).
            let from = self.strokes[n - 1].points.len().saturating_sub(1);
            self.draw_footprint(&strokes[n - 1], from);
        } else if prefix_kept && old + 1 == n && strokes[old - 1] == self.strokes[old - 1] {
            // A new stroke after the old ones: the old last stroke joins the history.
            let previous = self.strokes[old - 1].erase;
            fold(&mut self.alpha0, &mut self.beta0, &self.last, previous);
            self.last.iter_mut().for_each(|s| *s = 0.0);
            self.draw_footprint(&strokes[n - 1], 0);
        } else {
            // Anything else — an undo, a stroke removed, a setting changed: redraw all.
            self.alpha0.iter_mut().for_each(|a| *a = 1.0);
            self.beta0.iter_mut().for_each(|b| *b = 0.0);
            self.last.iter_mut().for_each(|s| *s = 0.0);
            if let Some((last, earlier)) = strokes.split_last() {
                for stroke in earlier {
                    self.draw_footprint(stroke, 0);
                    fold(&mut self.alpha0, &mut self.beta0, &self.last, stroke.erase);
                    self.last.iter_mut().for_each(|s| *s = 0.0);
                }
                self.draw_footprint(last, 0);
            }
        }
        self.strokes = strokes.to_vec();

        // Combine the history with the last stroke.
        let mut alpha = self.alpha0.clone();
        let mut beta = self.beta0.clone();
        if let Some(last) = strokes.last() {
            fold(&mut alpha, &mut beta, &self.last, last.erase);
        }
        self.combined = Arc::new(BrushRaster {
            w: self.w,
            h: self.h,
            alpha,
            beta,
        });
        self.combined.clone()
    }

    /// Grows the last-stroke footprint with `stroke`'s segments from point `from` on.
    fn draw_footprint(&mut self, stroke: &Stroke, from: usize) {
        let (w, h) = (self.w as f32, self.h as f32);
        let long = w.max(h);
        let r = (stroke.radius.clamp(0.0005, 0.5) * long).max(0.5);
        let feather = stroke.feather.clamp(0.0, 1.0);
        let inner = r * (1.0 - feather);
        let span = (r - inner).max(1e-6);
        let flow = stroke.flow.clamp(0.0, 1.0);
        let px: Vec<(f32, f32)> = stroke.points.iter().map(|p| (p[0] * w, p[1] * h)).collect();
        if px.is_empty() {
            return;
        }
        // A single point is a segment of no length: a dab.
        let segments: Vec<((f32, f32), (f32, f32))> = if px.len() == 1 {
            vec![(px[0], px[0])]
        } else {
            px.windows(2).skip(from).map(|s| (s[0], s[1])).collect()
        };
        let row_len = self.w as usize;
        for &(a, b) in &segments {
            self.segments_drawn += 1;
            let x_lo = ((a.0.min(b.0) - r).floor().max(0.0)) as usize;
            let x_hi = ((a.0.max(b.0) + r).ceil().min(w)) as usize;
            let y_lo = ((a.1.min(b.1) - r).floor().max(0.0)) as usize;
            let y_hi = ((a.1.max(b.1) + r).ceil().min(h)) as usize;
            if x_lo >= x_hi || y_lo >= y_hi {
                continue;
            }
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let len2 = dx * dx + dy * dy;
            self.last[y_lo * row_len..y_hi * row_len]
                .par_chunks_exact_mut(row_len)
                .enumerate()
                .for_each(|(j, row)| {
                    let qy = (y_lo + j) as f32 + 0.5;
                    for (x, s) in row.iter_mut().enumerate().take(x_hi).skip(x_lo) {
                        let qx = x as f32 + 0.5;
                        let t = if len2 > 0.0 {
                            (((qx - a.0) * dx + (qy - a.1) * dy) / len2).clamp(0.0, 1.0)
                        } else {
                            0.0
                        };
                        let (ex, ey) = (qx - (a.0 + t * dx), qy - (a.1 + t * dy));
                        let d = (ex * ex + ey * ey).sqrt();
                        if d >= r {
                            continue;
                        }
                        let fall = if d <= inner {
                            1.0
                        } else {
                            let t = ((d - inner) / span).clamp(0.0, 1.0);
                            1.0 - t * t * (3.0 - 2.0 * t)
                        };
                        let v = fall * flow;
                        if v > *s {
                            *s = v;
                        }
                    }
                });
        }
    }
}

/// Folds one stroke's footprint into `α, β`: add `c ↦ c(1−s)+s`, erase `c ↦ c(1−s)`.
fn fold(alpha: &mut [f32], beta: &mut [f32], s: &[f32], erase: bool) {
    alpha
        .par_iter_mut()
        .zip(beta.par_iter_mut())
        .zip(s.par_iter())
        .for_each(|((a, b), &s)| {
            if s > 0.0 {
                *a *= 1.0 - s;
                *b = *b * (1.0 - s) + if erase { 0.0 } else { s };
            }
        });
}

/// The rasters for `strokes` over a stored frame of `stored_w` × `stored_h`, drawn at most
/// `MAX_RASTER_EDGE` on the long edge: what an export uses.
pub fn rasterise(strokes: &[Stroke], stored_w: u32, stored_h: u32) -> Arc<BrushRaster> {
    let long = stored_w.max(stored_h).max(1);
    let (w, h) = if long > MAX_RASTER_EDGE {
        let k = MAX_RASTER_EDGE as f32 / long as f32;
        (
            ((stored_w as f32 * k).round() as u32).max(1),
            ((stored_h as f32 * k).round() as u32).max(1),
        )
    } else {
        (stored_w.max(1), stored_h.max(1))
    };
    BrushCache::new(w, h).update(strokes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stroke(points: &[[f32; 2]], radius: f32, erase: bool) -> Stroke {
        Stroke {
            points: points.to_vec(),
            radius,
            feather: 0.5,
            flow: 1.0,
            erase,
        }
    }

    fn line(n: usize, y: f32) -> Vec<[f32; 2]> {
        (0..n)
            .map(|i| [0.1 + 0.8 * i as f32 / (n - 1).max(1) as f32, y])
            .collect()
    }

    #[test]
    fn an_added_stroke_covers_its_path_and_nothing_far_from_it() {
        let r = rasterise(&[stroke(&line(20, 0.5), 0.05, false)], 400, 300);
        assert!(r.apply(0.0, 0.5, 0.5) > 0.99);
        assert_eq!(r.apply(0.0, 0.5, 0.9), 0.0);
        assert_eq!(
            r.apply(0.3, 0.5, 0.9),
            0.3,
            "elsewhere the mask's own coverage is kept"
        );
    }

    #[test]
    fn erasing_a_stroke_entirely_restores_the_coverage_exactly() {
        let path = line(30, 0.4);
        let add = stroke(&path, 0.04, false);
        let mut erase = stroke(&path, 0.08, true);
        erase.feather = 0.0;
        let r = rasterise(&[add, erase], 300, 200);
        for i in 0..300 {
            for j in 0..200 {
                let (u, v) = ((i as f32 + 0.5) / 300.0, (j as f32 + 0.5) / 200.0);
                assert_eq!(r.apply(0.0, u, v), 0.0, "at ({i}, {j})");
            }
        }
    }

    #[test]
    fn painting_over_an_erased_area_brings_it_back() {
        let path = line(10, 0.5);
        let r = rasterise(
            &[
                stroke(&path, 0.05, false),
                stroke(&path, 0.05, true),
                stroke(&path, 0.05, false),
            ],
            200,
            200,
        );
        assert!(
            r.apply(0.0, 0.5, 0.5) > 0.99,
            "the last stroke wins, in order"
        );
    }

    #[test]
    fn a_stroke_painted_point_by_point_matches_one_drawn_whole() {
        let path = line(60, 0.45);
        let whole = rasterise(
            &[
                stroke(&[[0.2, 0.2]], 0.1, false),
                stroke(&path, 0.03, false),
            ],
            320,
            240,
        );

        let mut cache = BrushCache::new(320, 240);
        cache.update(&[stroke(&[[0.2, 0.2]], 0.1, false)]);
        let mut grown = cache.update(&[
            stroke(&[[0.2, 0.2]], 0.1, false),
            stroke(&path[..1], 0.03, false),
        ]);
        let before = cache.segments_drawn;
        for k in 2..=path.len() {
            grown = cache.update(&[
                stroke(&[[0.2, 0.2]], 0.1, false),
                stroke(&path[..k], 0.03, false),
            ]);
        }
        // Each new point drew one segment, never the stroke again.
        assert_eq!(cache.segments_drawn - before, path.len() - 1);
        for i in 0..32 {
            for j in 0..24 {
                let (u, v) = ((i as f32 + 0.5) / 32.0, (j as f32 + 0.5) / 24.0);
                assert!((grown.apply(0.2, u, v) - whole.apply(0.2, u, v)).abs() < 1e-6);
            }
        }
    }

    #[test]
    fn an_export_draws_at_most_the_capped_size() {
        let r = rasterise(&[stroke(&[[0.5, 0.5]], 0.1, false)], 7360, 4912);
        assert_eq!((r.w, r.h), (4096, 2734));
        assert!(r.apply(0.0, 0.5, 0.5) > 0.99);
    }

    #[test]
    fn a_stroke_round_trips_through_json_with_defaults() {
        let s: Stroke = serde_json::from_str(r#"{"points":[[0.1,0.2]],"radius":0.05}"#).unwrap();
        assert_eq!((s.feather, s.flow, s.erase), (0.5, 1.0, false));
    }
}
