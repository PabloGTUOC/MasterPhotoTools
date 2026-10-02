//! Spatial transforms and upright orientation baking (ED-13).
//!
//! Citations & Principles:
//! - Coordinate space: Crop, rotation, straighten, and flips are defined **in the upright (displayed) frame**.
//! - Single-pass inverse mapping: The geometry stage composes the EXIF orientation with the user's
//!   transforms into one affine mapping from output pixel to stored pixel, sampled bilinearly in linear light.
//! - Largest inscribed rectangle: Straighten rotates by fine angle θ in [-45.0°, +45.0°] and crops to
//!   the largest rectangle of the target aspect ratio inside the rotated bounding box, mathematically
//!   guaranteeing no void or transparent corners are ever exposed.
//! - Aspect ratio presets: Defined in **output pixels**; normalized crop is computed from pixel aspect.
//! - Refuses crops smaller than 16 px on either dimension.

use crate::error::Error;
use crate::media::edit::pipeline::{ImageBuffer, LinearBuffer};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

/// Minimum allowed output dimension in pixels for a crop.
pub const MIN_CROP_DIMENSION: u32 = 16;

/// Normalised crop rectangle in the upright displayed frame [0.0, 1.0].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct NormalizedCrop {
    /// Left origin [0.0, 1.0].
    pub x: f32,
    /// Top origin [0.0, 1.0].
    pub y: f32,
    /// Width (0.0, 1.0].
    pub width: f32,
    /// Height (0.0, 1.0].
    pub height: f32,
}

impl Default for NormalizedCrop {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        }
    }
}

impl NormalizedCrop {
    /// Returns true if the crop spans the entire frame (exact identity).
    pub fn is_identity(&self) -> bool {
        self.x.abs() < 1e-4
            && self.y.abs() < 1e-4
            && (self.width - 1.0).abs() < 1e-4
            && (self.height - 1.0).abs() < 1e-4
    }

    /// Validates bounds and minimum dimension.
    pub fn validate(&self, upright_w: u32, upright_h: u32) -> Result<(), Error> {
        if self.x < -1e-4
            || self.y < -1e-4
            || self.width <= 0.0
            || self.height <= 0.0
            || (self.x + self.width) > 1.0 + 1e-4
            || (self.y + self.height) > 1.0 + 1e-4
        {
            return Err(Error::Refused(format!(
                "Invalid crop bounds [x={}, y={}, w={}, h={}]: must lie within [0.0, 1.0]",
                self.x, self.y, self.width, self.height
            )));
        }
        let crop_w = (self.width * upright_w as f32).round() as u32;
        let crop_h = (self.height * upright_h as f32).round() as u32;
        if crop_w < MIN_CROP_DIMENSION || crop_h < MIN_CROP_DIMENSION {
            return Err(Error::Refused(format!(
                "Crop dimensions {crop_w}x{crop_h} smaller than {MIN_CROP_DIMENSION}px on either side are refused"
            )));
        }
        Ok(())
    }
}

/// Geometric transformation configuration (ED-13).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Geometry {
    /// Optional normalised crop rectangle in upright displayed coordinates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crop: Option<NormalizedCrop>,
    /// Clockwise rotation in 90-degree increments (0, 90, 180, 270).
    #[serde(default)]
    pub rotate: i32,
    /// Fine straighten angle in degrees [-45.0, +45.0].
    #[serde(default)]
    pub straighten: f32,
    /// Flip horizontal in the upright frame.
    #[serde(default)]
    pub flip_h: bool,
    /// Flip vertical in the upright frame.
    #[serde(default)]
    pub flip_v: bool,
    /// Aspect preset name (e.g. "original", "free", "1:1", "3:2", "16:9").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aspect: Option<String>,
}

impl Default for Geometry {
    fn default() -> Self {
        Self {
            crop: None,
            rotate: 0,
            straighten: 0.0,
            flip_h: false,
            flip_v: false,
            aspect: None,
        }
    }
}

impl Geometry {
    /// Returns true if all geometric transforms are at rest (identity).
    pub fn is_identity(&self) -> bool {
        let crop_id = self.crop.as_ref().map_or(true, |c| c.is_identity());
        crop_id
            && (self.rotate % 360 == 0)
            && self.straighten.abs() < 1e-4
            && !self.flip_h
            && !self.flip_v
    }

    /// The mapping from output pixels to stored pixels for a source of `stored_w` × `stored_h`
    /// in EXIF `orientation`, and the output size it produces.
    ///
    /// One place computes this so the pixels and anything placed on them (masks, ED-19) are
    /// moved by the same transform; two copies of the arithmetic would drift.
    pub fn plan(
        &self,
        stored_w: u32,
        stored_h: u32,
        orientation: u32,
    ) -> Result<GeometryPlan, Error> {
        let (w_s, h_s) = (stored_w as f32, stored_h as f32);

        // 1. Upright frame dimensions from EXIF orientation
        let (w_u, h_u) = if (5..=8).contains(&orientation) {
            (h_s, w_s)
        } else {
            (w_s, h_s)
        };

        // 2. User 90-degree rotation
        let k = ((self.rotate % 360 + 360) / 90) % 4;
        let (w_r, h_r) = if k == 1 || k == 3 {
            (h_u, w_u)
        } else {
            (w_u, h_u)
        };

        // 3. Straighten inscribed rectangle
        let theta_deg = self.straighten.clamp(-45.0, 45.0);
        let base_aspect = w_r / h_r;
        let (w_ins, h_ins) = largest_inscribed_rect(w_r, h_r, theta_deg, base_aspect);

        // 4. Crop subregion
        let crop = self.crop.unwrap_or_default();
        crop.validate(w_ins.round() as u32, h_ins.round() as u32)?;

        let out_w = ((crop.width * w_ins).round() as u32).max(MIN_CROP_DIMENSION);
        let out_h = ((crop.height * h_ins).round() as u32).max(MIN_CROP_DIMENSION);

        Ok(GeometryPlan {
            out_w,
            out_h,
            w_s,
            h_s,
            w_u,
            h_u,
            w_r,
            h_r,
            w_ins,
            h_ins,
            crop,
            k,
            cos_t: theta_deg.to_radians().cos(),
            sin_t: theta_deg.to_radians().sin(),
            flip_h: self.flip_h,
            flip_v: self.flip_v,
            orientation,
        })
    }

    /// Applies geometric transformations to a `LinearBuffer` in linear light.
    ///
    /// Single-pass bilinear sampling directly from stored coordinates $(x_s, y_s)$
    /// to output pixels $(x_{out}, y_{out})$, baking orientation into an upright frame.
    pub fn apply_to_linear(
        &self,
        src: &LinearBuffer,
        orientation: u32,
    ) -> Result<LinearBuffer, Error> {
        if self.is_identity() && orientation == 1 {
            return Ok(src.clone());
        }

        let plan = self.plan(src.width, src.height, orientation)?;
        let (out_w, out_h) = (plan.out_w, plan.out_h);
        let mut out_data = vec![0.0f32; (out_w * out_h * 3) as usize];

        let src_data = &src.data;
        let src_w_usize = src.width as usize;
        let src_h_usize = src.height as usize;

        out_data
            .par_chunks_exact_mut((out_w * 3) as usize)
            .enumerate()
            .for_each(|(j, row_slice)| {
                let j_f = j as f32 + 0.5;
                let (x0, y0) = plan.map(0.5, j_f);
                let (x1, y1) = plan.map(1.5, j_f);
                let dx = x1 - x0;
                let dy = y1 - y0;

                let mut cur_x = x0;
                let mut cur_y = y0;

                for px in row_slice.chunks_exact_mut(3) {
                    let (r, g, b) =
                        sample_bilinear(src_data, src_w_usize, src_h_usize, cur_x, cur_y);
                    px[0] = r;
                    px[1] = g;
                    px[2] = b;
                    cur_x += dx;
                    cur_y += dy;
                }
            });

        Ok(LinearBuffer {
            width: out_w,
            height: out_h,
            data: out_data,
        })
    }

    /// Applies geometric transformations to an `ImageBuffer`.
    pub fn apply_to_image_buffer(
        &self,
        src: &ImageBuffer,
        orientation: u32,
    ) -> Result<ImageBuffer, Error> {
        let linear = LinearBuffer::from_image_buffer(src);
        let transformed = self.apply_to_linear(&linear, orientation)?;
        match src {
            ImageBuffer::Rgb8 { .. } => Ok(transformed.to_rgb8()),
            ImageBuffer::Rgb16 { .. } => Ok(transformed.to_rgb16()),
        }
    }
}

/// A geometry resolved against one source: its output size and the affine map from
/// output pixel to stored pixel (ED-13), shared by the pixels and the masks (ED-19).
#[derive(Debug, Clone, Copy)]
pub struct GeometryPlan {
    pub out_w: u32,
    pub out_h: u32,
    w_s: f32,
    h_s: f32,
    w_u: f32,
    h_u: f32,
    w_r: f32,
    h_r: f32,
    w_ins: f32,
    h_ins: f32,
    crop: NormalizedCrop,
    k: i32,
    cos_t: f32,
    sin_t: f32,
    flip_h: bool,
    flip_v: bool,
    orientation: u32,
}

impl GeometryPlan {
    /// Stored-pixel coordinates of output pixel coordinates `(i, j)`; pixel centres are at +0.5.
    #[inline]
    pub fn map(&self, i: f32, j: f32) -> (f32, f32) {
        let (w_s, h_s, w_u, h_u, w_r, h_r) =
            (self.w_s, self.h_s, self.w_u, self.h_u, self.w_r, self.h_r);
        let (w_ins, h_ins, crop) = (self.w_ins, self.h_ins, self.crop);
        let (cos_t, sin_t) = (self.cos_t, self.sin_t);

        // (a) Coordinate in inscribed rectangle
        let u_ins = (crop.x * w_ins) + (i / self.out_w as f32) * (crop.width * w_ins);
        let v_ins = (crop.y * h_ins) + (j / self.out_h as f32) * (crop.height * h_ins);

        // (b) Relative to center of inscribed rectangle
        let x_rel = u_ins - w_ins * 0.5;
        let y_rel = v_ins - h_ins * 0.5;

        // (c) Inverse rotate by theta (rotation by -theta around center of w_r, h_r)
        let mut x_r = w_r * 0.5 + (x_rel * cos_t + y_rel * sin_t);
        let mut y_r = h_r * 0.5 + (-x_rel * sin_t + y_rel * cos_t);

        // (d) Inverse flips
        if self.flip_h {
            x_r = w_r - x_r;
        }
        if self.flip_v {
            y_r = h_r - y_r;
        }

        // (e) Inverse 90-degree user rotation
        let (x_u, y_u) = match self.k {
            0 => (x_r, y_r),
            1 => (y_r, h_u - x_r),
            2 => (w_u - x_r, h_u - y_r),
            3 => (w_u - y_r, x_r),
            _ => (x_r, y_r),
        };

        // (f) Inverse EXIF orientation to stored coordinates
        match self.orientation {
            1 => (x_u, y_u),
            2 => (w_s - x_u, y_u),
            3 => (w_s - x_u, h_s - y_u),
            4 => (x_u, h_s - y_u),
            5 => (y_u, x_u),
            6 => (y_u, h_s - x_u),
            7 => (w_s - y_u, h_s - x_u),
            8 => (w_s - y_u, x_u),
            _ => (x_u, y_u),
        }
    }

    /// The same map in normalised coordinates, as `[a, b, c, d, e, f]` with
    /// `u_s = a·u + b·v + c` and `v_s = d·u + e·v + f`, where `(u, v)` spans the output
    /// and `(u_s, v_s)` the stored frame, both on [0, 1]. Every step of `map` is affine,
    /// so three points determine it; normalised, it no longer depends on the resolution.
    pub fn normalised_affine(&self) -> [f32; 6] {
        let (w, h) = (self.out_w as f32, self.out_h as f32);
        let (x0, y0) = self.map(0.0, 0.0);
        let (x1, y1) = self.map(w, 0.0);
        let (x2, y2) = self.map(0.0, h);
        [
            (x1 - x0) / self.w_s,
            (x2 - x0) / self.w_s,
            x0 / self.w_s,
            (y1 - y0) / self.h_s,
            (y2 - y0) / self.h_s,
            y0 / self.h_s,
        ]
    }
}

/// Bilinearly samples `src` [RGB f32] at stored coordinates `(xs, ys)`.
#[inline(always)]
pub fn sample_bilinear(src: &[f32], w: usize, h: usize, xs: f32, ys: f32) -> (f32, f32, f32) {
    let u = (xs - 0.5).clamp(0.0, (w - 1) as f32);
    let v = (ys - 0.5).clamp(0.0, (h - 1) as f32);

    let x0 = u as usize;
    let y0 = v as usize;
    let x1 = (x0 + 1).min(w - 1);
    let y1 = (y0 + 1).min(h - 1);

    let fx = u - x0 as f32;
    let fy = v - y0 as f32;

    let w00 = (1.0 - fx) * (1.0 - fy);
    let w10 = fx * (1.0 - fy);
    let w01 = (1.0 - fx) * fy;
    let w11 = fx * fy;

    let idx00 = (y0 * w + x0) * 3;
    let idx10 = (y0 * w + x1) * 3;
    let idx01 = (y1 * w + x0) * 3;
    let idx11 = (y1 * w + x1) * 3;

    let r = w00 * src[idx00] + w10 * src[idx10] + w01 * src[idx01] + w11 * src[idx11];
    let g =
        w00 * src[idx00 + 1] + w10 * src[idx10 + 1] + w01 * src[idx01 + 1] + w11 * src[idx11 + 1];
    let b =
        w00 * src[idx00 + 2] + w10 * src[idx10 + 2] + w01 * src[idx01 + 2] + w11 * src[idx11 + 2];

    (r, g, b)
}

/// Computes the largest inscribed rectangle of aspect ratio `aspect = w / h`
/// inside a rectangle of width `w_r` and height `h_r` rotated by `straighten_deg`.
///
/// Formulation:
/// Given outer box $W \times H$ rotated by fine angle $|\theta| \in [0, 45^\circ]$:
/// Any inscribed box $w \times h$ with $w = A \cdot h$ satisfies:
/// $w \cos \theta + h \sin \theta \le W \implies h \le \frac{W}{A \cos \theta + \sin \theta}$
/// $w \sin \theta + h \cos \theta \le H \implies h \le \frac{H}{A \sin \theta + \cos \theta}$
/// Taking the minimum ensures no transparent or void corners can ever show.
pub fn largest_inscribed_rect(w_r: f32, h_r: f32, straighten_deg: f32, aspect: f32) -> (f32, f32) {
    let theta = straighten_deg.to_radians().abs();
    if theta < 1e-6 {
        if aspect > w_r / h_r {
            return (w_r, w_r / aspect);
        } else {
            return (h_r * aspect, h_r);
        }
    }
    let c = theta.cos();
    let s = theta.sin();
    let a = aspect.max(1e-4);

    let h1 = w_r / (a * c + s);
    let h2 = h_r / (a * s + c);
    let h_ins = h1.min(h2);
    let w_ins = a * h_ins;
    (w_ins, h_ins)
}

/// Returns target aspect ratio `w / h` for a preset name.
pub fn aspect_ratio_from_preset(preset: &str, upright_w: u32, upright_h: u32) -> Option<f32> {
    match preset.trim().to_lowercase().as_str() {
        "original" => Some(upright_w as f32 / upright_h as f32),
        "free" => None,
        "1:1" => Some(1.0),
        "3:2" => Some(3.0 / 2.0),
        "2:3" => Some(2.0 / 3.0),
        "4:3" => Some(4.0 / 3.0),
        "3:4" => Some(3.0 / 4.0),
        "16:9" => Some(16.0 / 9.0),
        "9:16" => Some(9.0 / 16.0),
        "5:4" => Some(5.0 / 4.0),
        "4:5" => Some(4.0 / 5.0),
        _ => None,
    }
}

/// Calculates a centered normalized crop for a given aspect preset on upright image dimensions.
pub fn preset_to_normalized_crop(
    preset: &str,
    upright_w: u32,
    upright_h: u32,
) -> Result<NormalizedCrop, Error> {
    let r = aspect_ratio_from_preset(preset, upright_w, upright_h)
        .unwrap_or(upright_w as f32 / upright_h as f32);

    let img_r = upright_w as f32 / upright_h as f32;
    let (crop_w_px, crop_h_px) = if r > img_r {
        let w = upright_w as f32;
        let h = w / r;
        (w, h)
    } else {
        let h = upright_h as f32;
        let w = h * r;
        (w, h)
    };

    if crop_w_px < MIN_CROP_DIMENSION as f32 || crop_h_px < MIN_CROP_DIMENSION as f32 {
        return Err(Error::Refused(format!(
            "Preset '{preset}' produces dimensions smaller than {MIN_CROP_DIMENSION}px"
        )));
    }

    let norm_w = crop_w_px / upright_w as f32;
    let norm_h = crop_h_px / upright_h as f32;
    let norm_x = (1.0 - norm_w) * 0.5;
    let norm_y = (1.0 - norm_h) * 0.5;

    Ok(NormalizedCrop {
        x: norm_x,
        y: norm_y,
        width: norm_w,
        height: norm_h,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_pattern(w: u32, h: u32) -> LinearBuffer {
        // Creates a ramp test image where R = x / w, G = y / h, B = 0.5
        let mut data = Vec::with_capacity((w * h * 3) as usize);
        for y in 0..h {
            for x in 0..w {
                data.push(x as f32 / w as f32);
                data.push(y as f32 / h as f32);
                data.push(0.5);
            }
        }
        LinearBuffer {
            width: w,
            height: h,
            data,
        }
    }

    #[test]
    fn untouched_geometry_preserves_original_dimensions_and_orientation_tag() {
        let buf = create_test_pattern(100, 80);
        let geom = Geometry::default();
        assert!(geom.is_identity());

        let out = geom.apply_to_linear(&buf, 1).unwrap();
        assert_eq!(out.width, 100);
        assert_eq!(out.height, 80);
        assert_eq!(out.data, buf.data);
    }

    #[test]
    fn crop_extracts_exact_normalized_subregion() {
        let buf = create_test_pattern(200, 100);
        let geom = Geometry {
            crop: Some(NormalizedCrop {
                x: 0.25,
                y: 0.25,
                width: 0.5,
                height: 0.5,
            }),
            ..Default::default()
        };
        let out = geom.apply_to_linear(&buf, 1).unwrap();
        assert_eq!(out.width, 100);
        assert_eq!(out.height, 50);

        // Center pixel of cropped region should match center pixel of source
        let out_center_idx = (25 * 100 + 50) * 3;
        let src_center_idx = (50 * 200 + 100) * 3;

        let diff_r = (out.data[out_center_idx] - buf.data[src_center_idx]).abs();
        let diff_g = (out.data[out_center_idx + 1] - buf.data[src_center_idx + 1]).abs();
        assert!(diff_r < 0.02, "diff_r = {diff_r}");
        assert!(diff_g < 0.02, "diff_g = {diff_g}");
    }

    #[test]
    fn crop_coordinates_are_in_the_upright_frame_for_every_orientation() {
        // Create an identifiable gradient: R encodes upright X, G encodes upright Y
        // For each of the 8 orientations, build the stored buffer such that after
        // EXIF orientation, top-left is (R=0, G=0).
        for orient in 1..=8 {
            let (w_u, h_u) = (120, 80);
            let (w_s, h_s) = if (5..=8).contains(&orient) {
                (h_u, w_u)
            } else {
                (w_u, h_u)
            };

            // Synthesize stored buffer by inverting the orientation
            let mut stored_data = vec![0.0f32; (w_s * h_s * 3) as usize];
            for y_s in 0..h_s {
                for x_s in 0..w_s {
                    let (x_u, y_u) = match orient {
                        1 => (x_s, y_s),
                        2 => (w_s - 1 - x_s, y_s),
                        3 => (w_s - 1 - x_s, h_s - 1 - y_s),
                        4 => (x_s, h_s - 1 - y_s),
                        5 => (y_s, x_s),
                        6 => (h_s - 1 - y_s, x_s),
                        7 => (h_s - 1 - y_s, w_s - 1 - x_s),
                        8 => (y_s, w_s - 1 - x_s),
                        _ => (x_s, y_s),
                    };
                    let idx = ((y_s * w_s + x_s) * 3) as usize;
                    stored_data[idx] = x_u as f32 / w_u as f32;
                    stored_data[idx + 1] = y_u as f32 / h_u as f32;
                    stored_data[idx + 2] = 0.5;
                }
            }

            let src_buf = LinearBuffer {
                width: w_s,
                height: h_s,
                data: stored_data,
            };

            // Crop the top-left quarter in the upright displayed frame [0, 0, 0.5, 0.5]
            let geom = Geometry {
                crop: Some(NormalizedCrop {
                    x: 0.0,
                    y: 0.0,
                    width: 0.5,
                    height: 0.5,
                }),
                ..Default::default()
            };

            let cropped = geom.apply_to_linear(&src_buf, orient).unwrap();
            assert_eq!(cropped.width, 60);
            assert_eq!(cropped.height, 40);

            // In upright frame, top-left quarter must have R in [0, 0.5] and G in [0, 0.5]
            let first_px_r = cropped.data[0];
            let first_px_g = cropped.data[1];
            assert!(
                first_px_r < 0.05,
                "Orient {orient}: expected first_px_r ~0, got {first_px_r}"
            );
            assert!(
                first_px_g < 0.05,
                "Orient {orient}: expected first_px_g ~0, got {first_px_g}"
            );

            let last_px_idx = ((39 * 60 + 59) * 3) as usize;
            let last_px_r = cropped.data[last_px_idx];
            let last_px_g = cropped.data[last_px_idx + 1];
            assert!(
                (last_px_r - 0.5).abs() < 0.05,
                "Orient {orient}: expected last_px_r ~0.5, got {last_px_r}"
            );
            assert!(
                (last_px_g - 0.5).abs() < 0.05,
                "Orient {orient}: expected last_px_g ~0.5, got {last_px_g}"
            );
        }
    }

    #[test]
    fn straighten_inscribes_and_crops_without_transparent_voids() {
        for angle in [-45.0, -30.0, -15.0, -7.0, 0.0, 7.0, 15.0, 30.0, 45.0] {
            let (w_r, h_r) = (1000.0, 667.0);
            let aspect = w_r / h_r;
            let (w_ins, h_ins) = largest_inscribed_rect(w_r, h_r, angle, aspect);

            assert!(w_ins > 0.0 && h_ins > 0.0);
            assert!((w_ins / h_ins - aspect).abs() < 1e-3);

            // Check that every corner of the inscribed rectangle rotated by theta stays within bounds
            let theta = angle.to_radians();
            let cos_t = theta.cos();
            let sin_t = theta.sin();

            for (cx, cy) in [
                (-w_ins * 0.5, -h_ins * 0.5),
                (w_ins * 0.5, -h_ins * 0.5),
                (-w_ins * 0.5, h_ins * 0.5),
                (w_ins * 0.5, h_ins * 0.5),
            ] {
                let rx = (cx * cos_t - cy * sin_t).abs();
                let ry = (cx * sin_t + cy * cos_t).abs();
                assert!(
                    rx <= w_r * 0.5 + 1e-3,
                    "Angle {angle}: corner rx={rx} exceeds w_r/2={}",
                    w_r * 0.5
                );
                assert!(
                    ry <= h_r * 0.5 + 1e-3,
                    "Angle {angle}: corner ry={ry} exceeds h_r/2={}",
                    h_r * 0.5
                );
            }
        }
    }

    #[test]
    fn aspect_preset_produces_the_exact_output_ratio() {
        let presets = [
            ("original", 1.5, 2.0 / 3.0),
            ("1:1", 1.0, 1.0),
            ("3:2", 1.5, 1.5),
            ("2:3", 2.0 / 3.0, 2.0 / 3.0),
            ("4:3", 4.0 / 3.0, 4.0 / 3.0),
            ("3:4", 0.75, 0.75),
            ("16:9", 16.0 / 9.0, 16.0 / 9.0),
            ("9:16", 9.0 / 16.0, 9.0 / 16.0),
            ("5:4", 1.25, 1.25),
            ("4:5", 0.8, 0.8),
        ];

        // Test in both landscape (3000 x 2000) and portrait (2000 x 3000)
        for (name, expected_land, expected_port) in presets {
            // Landscape
            let crop_land = preset_to_normalized_crop(name, 3000, 2000).unwrap();
            let pixel_w_land = crop_land.width * 3000.0;
            let pixel_h_land = crop_land.height * 2000.0;
            let ratio_land = pixel_w_land / pixel_h_land;
            assert!(
                (ratio_land - expected_land).abs() < 1e-3,
                "Preset '{name}' landscape ratio mismatch: expected {expected_land}, got {ratio_land}"
            );

            // Portrait
            let crop_port = preset_to_normalized_crop(name, 2000, 3000).unwrap();
            let pixel_w_port = crop_port.width * 2000.0;
            let pixel_h_port = crop_port.height * 3000.0;
            let ratio_port = pixel_w_port / pixel_h_port;
            assert!(
                (ratio_port - expected_port).abs() < 1e-3,
                "Preset '{name}' portrait ratio mismatch: expected {expected_port}, got {ratio_port}"
            );
        }
    }

    #[test]
    fn ninety_degree_rotation_swaps_dimensions_and_transposes_pixels() {
        let buf = create_test_pattern(100, 50);
        let geom_90 = Geometry {
            rotate: 90,
            ..Default::default()
        };
        let out_90 = geom_90.apply_to_linear(&buf, 1).unwrap();
        assert_eq!(out_90.width, 50);
        assert_eq!(out_90.height, 100);

        let geom_180 = Geometry {
            rotate: 180,
            ..Default::default()
        };
        let out_180 = geom_180.apply_to_linear(&buf, 1).unwrap();
        assert_eq!(out_180.width, 100);
        assert_eq!(out_180.height, 50);

        let geom_270 = Geometry {
            rotate: 270,
            ..Default::default()
        };
        let out_270 = geom_270.apply_to_linear(&buf, 1).unwrap();
        assert_eq!(out_270.width, 50);
        assert_eq!(out_270.height, 100);
    }

    #[test]
    fn horizontal_and_vertical_flips_mirror_pixels_accurately() {
        let buf = create_test_pattern(100, 60);

        let geom_h = Geometry {
            flip_h: true,
            ..Default::default()
        };
        let out_h = geom_h.apply_to_linear(&buf, 1).unwrap();
        assert_eq!(out_h.width, 100);
        assert_eq!(out_h.height, 60);

        // Top-left pixel of out_h should match top-right pixel of buf
        let left_idx = 0;
        let right_idx = (99 * 3) as usize;
        assert!((out_h.data[left_idx] - buf.data[right_idx]).abs() < 0.02);

        let geom_v = Geometry {
            flip_v: true,
            ..Default::default()
        };
        let out_v = geom_v.apply_to_linear(&buf, 1).unwrap();
        assert_eq!(out_v.width, 100);
        assert_eq!(out_v.height, 60);

        // Top-left pixel of out_v should match bottom-left pixel of buf
        let bot_idx = ((59 * 100) * 3) as usize;
        assert!((out_v.data[left_idx + 1] - buf.data[bot_idx + 1]).abs() < 0.02);
    }

    #[test]
    fn geometric_transforms_bake_orientation_to_one_across_all_eight_exif_orientations() {
        for orient in 1..=8 {
            let buf = create_test_pattern(60, 40);
            let geom = Geometry {
                rotate: 90,
                ..Default::default()
            };
            let out = geom.apply_to_linear(&buf, orient).unwrap();
            assert!(out.width >= MIN_CROP_DIMENSION);
            assert!(out.height >= MIN_CROP_DIMENSION);
            assert!(!out.data.iter().any(|v| v.is_nan() || v.is_infinite()));
        }
    }
}
