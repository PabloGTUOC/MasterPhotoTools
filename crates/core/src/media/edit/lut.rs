//! 3D Look-Up Table (LUT) parser and tetrahedral interpolator (ED-2).
//!
//! # Clean-Room Format Specifications
//!
//! All parsers are implemented strictly from public format descriptions:
//!
//! - **Adobe Cube LUT Specification 1.0**:
//!   Adobe Systems Incorporated, *Cube LUT Specification v1.0* (2013).
//!   Defines `.cube` 3D tables with keyword lines (`TITLE`, `LUT_3D_SIZE`,
//!   `DOMAIN_MIN`, `DOMAIN_MAX`). Ordering: Red varies fastest, then Green, then Blue.
//!   1D LUTs (`LUT_1D_SIZE`) are rejected with a clear refusal message.
//!
//! - **Autodesk Flame / Discreet Lustre 3DL Specification**:
//!   Autodesk, Inc., *Discreet 3D LUT format specification (ASCII 3DL)*.
//!   Defines grid mesh lines and bit-depth scaling.
//!   Ordering: Blue varies fastest, then Green, then Red (outer loop R, middle loop G, inner loop B).
//!
//! - **HALD CLUT Specification**:
//!   Eskil Steenberg / Quel Solaar, *HALD CLUT image specification*.
//!   Square 8-bit and 16-bit PNG image of dimensions L³ × L³ representing
//!   a 3D cube lattice of size N = L² (N³ = L⁶ pixels).
//!   Ordering: Row-major scanline order is Red varies fastest, then Green, then Blue.

use crate::error::Error;
use sha2::{Digest, Sha256};
use std::path::Path;

/// Supported minimum 3D LUT lattice dimension.
pub const MIN_LUT_SIZE: usize = 2;

/// Supported maximum 3D LUT lattice dimension.
/// A 256³ lattice requires ~201 MB of f32 memory. Files declaring larger sizes
/// are rejected before memory allocation to prevent out-of-memory denial of service.
pub const MAX_LUT_SIZE: usize = 256;

/// Parsed 3D Look-Up Table with domain boundaries and content hash.
#[derive(Debug, Clone, PartialEq)]
pub struct Lut {
    pub title: Option<String>,
    pub size: usize,
    pub domain_min: [f32; 3],
    pub domain_max: [f32; 3],
    pub sha256: String,
    /// Lattice nodes in canonical Red-fastest order:
    /// `index = r + g * size + b * size * size`.
    pub data: Vec<[f32; 3]>,
}

fn compute_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let result = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for b in result {
        use std::fmt::Write;
        let _ = write!(&mut hex, "{b:02x}");
    }
    hex
}

impl Lut {
    /// Loads and parses a 3D LUT from a file on disk.
    pub fn from_file(path: &Path) -> Result<Self, Error> {
        let bytes = std::fs::read(path)?;
        let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("lut");
        Self::from_bytes(filename, &bytes)
    }

    /// Parses a 3D LUT from in-memory bytes, inferring format from filename extension.
    pub fn from_bytes(filename: &str, bytes: &[u8]) -> Result<Self, Error> {
        let lower = filename.to_ascii_lowercase();
        if lower.ends_with(".cube") {
            Self::from_cube(filename, bytes)
        } else if lower.ends_with(".3dl") {
            Self::from_3dl(filename, bytes)
        } else if lower.ends_with(".png") {
            Self::from_hald_png(filename, bytes)
        } else {
            // Attempt cube, then 3dl, then hald png
            if let Ok(lut) = Self::from_cube(filename, bytes) {
                Ok(lut)
            } else if let Ok(lut) = Self::from_3dl(filename, bytes) {
                Ok(lut)
            } else if let Ok(lut) = Self::from_hald_png(filename, bytes) {
                Ok(lut)
            } else {
                Err(Error::Refused(format!(
                    "{filename}: unrecognized LUT format (expected .cube, .3dl, or HALD .png)"
                )))
            }
        }
    }

    /// Parses an Adobe `.cube` 3D LUT according to Adobe Cube LUT Specification 1.0.
    ///
    /// The table data is ordered such that Red varies fastest, then Green, then Blue.
    /// 1D LUTs (`LUT_1D_SIZE`) are explicitly refused.
    pub fn from_cube(filename: &str, bytes: &[u8]) -> Result<Self, Error> {
        let text = std::str::from_utf8(bytes)
            .map_err(|_| Error::Refused(format!("{filename}: file is not valid UTF-8 text")))?;

        let mut title: Option<String> = None;
        let mut size: Option<usize> = None;
        let mut domain_min = [0.0f32, 0.0f32, 0.0f32];
        let mut domain_max = [1.0f32, 1.0f32, 1.0f32];
        let mut data: Option<Vec<[f32; 3]>> = None;
        let mut line_no = 0usize;

        for line in text.lines() {
            line_no += 1;
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            // Header keywords
            if trimmed.starts_with("LUT_1D_SIZE") {
                return Err(Error::Refused(format!(
                    "{filename}:{line_no}: 1D LUTs are not supported: found LUT_1D_SIZE"
                )));
            }

            if let Some(rest) = trimmed.strip_prefix("LUT_3D_SIZE") {
                let parts: Vec<&str> = rest.split_whitespace().collect();
                if parts.is_empty() {
                    return Err(Error::Refused(format!(
                        "{filename}:{line_no}: missing size argument for LUT_3D_SIZE"
                    )));
                }
                let n: usize = parts[0].parse().map_err(|_| {
                    Error::Refused(format!(
                        "{filename}:{line_no}: invalid integer for LUT_3D_SIZE: '{}'",
                        parts[0]
                    ))
                })?;
                if !(MIN_LUT_SIZE..=MAX_LUT_SIZE).contains(&n) {
                    return Err(Error::Refused(format!(
                        "{filename}:{line_no}: LUT_3D_SIZE {n} is outside supported range [{MIN_LUT_SIZE}, {MAX_LUT_SIZE}]"
                    )));
                }
                size = Some(n);
                let capacity =
                    n.checked_mul(n)
                        .and_then(|p| p.checked_mul(n))
                        .ok_or_else(|| {
                            Error::Refused(format!("{filename}:{line_no}: LUT size overflow"))
                        })?;
                data = Some(Vec::with_capacity(capacity));
                continue;
            }

            if let Some(rest) = trimmed.strip_prefix("TITLE") {
                let t = rest.trim().trim_matches('"');
                title = Some(t.to_string());
                continue;
            }

            if let Some(rest) = trimmed.strip_prefix("DOMAIN_MIN") {
                let parts: Vec<&str> = rest.split_whitespace().collect();
                if parts.len() != 3 {
                    return Err(Error::Refused(format!(
                        "{filename}:{line_no}: DOMAIN_MIN requires 3 values, found {}",
                        parts.len()
                    )));
                }
                let r: f32 = parts[0].parse().map_err(|_| {
                    Error::Refused(format!("{filename}:{line_no}: invalid DOMAIN_MIN R"))
                })?;
                let g: f32 = parts[1].parse().map_err(|_| {
                    Error::Refused(format!("{filename}:{line_no}: invalid DOMAIN_MIN G"))
                })?;
                let b: f32 = parts[2].parse().map_err(|_| {
                    Error::Refused(format!("{filename}:{line_no}: invalid DOMAIN_MIN B"))
                })?;
                if !r.is_finite() || !g.is_finite() || !b.is_finite() {
                    return Err(Error::Refused(format!(
                        "{filename}:{line_no}: DOMAIN_MIN values must be finite"
                    )));
                }
                domain_min = [r, g, b];
                continue;
            }

            if let Some(rest) = trimmed.strip_prefix("DOMAIN_MAX") {
                let parts: Vec<&str> = rest.split_whitespace().collect();
                if parts.len() != 3 {
                    return Err(Error::Refused(format!(
                        "{filename}:{line_no}: DOMAIN_MAX requires 3 values, found {}",
                        parts.len()
                    )));
                }
                let r: f32 = parts[0].parse().map_err(|_| {
                    Error::Refused(format!("{filename}:{line_no}: invalid DOMAIN_MAX R"))
                })?;
                let g: f32 = parts[1].parse().map_err(|_| {
                    Error::Refused(format!("{filename}:{line_no}: invalid DOMAIN_MAX G"))
                })?;
                let b: f32 = parts[2].parse().map_err(|_| {
                    Error::Refused(format!("{filename}:{line_no}: invalid DOMAIN_MAX B"))
                })?;
                if !r.is_finite() || !g.is_finite() || !b.is_finite() {
                    return Err(Error::Refused(format!(
                        "{filename}:{line_no}: DOMAIN_MAX values must be finite"
                    )));
                }
                domain_max = [r, g, b];
                continue;
            }

            // Data lines
            let Some(n) = size else {
                return Err(Error::Refused(format!(
                    "{filename}:{line_no}: data encountered before LUT_3D_SIZE was defined"
                )));
            };

            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() != 3 {
                return Err(Error::Refused(format!(
                    "{filename}:{line_no}: expected 3 color components, found {}",
                    parts.len()
                )));
            }

            let r: f32 = parts[0].parse().map_err(|_| {
                Error::Refused(format!(
                    "{filename}:{line_no}: invalid float value '{}'",
                    parts[0]
                ))
            })?;
            let g: f32 = parts[1].parse().map_err(|_| {
                Error::Refused(format!(
                    "{filename}:{line_no}: invalid float value '{}'",
                    parts[1]
                ))
            })?;
            let b: f32 = parts[2].parse().map_err(|_| {
                Error::Refused(format!(
                    "{filename}:{line_no}: invalid float value '{}'",
                    parts[2]
                ))
            })?;

            if !r.is_finite() || !g.is_finite() || !b.is_finite() {
                return Err(Error::Refused(format!(
                    "{filename}:{line_no}: non-finite float value encountered"
                )));
            }

            let entries = data.as_mut().unwrap();
            let total_expected = n * n * n;
            if entries.len() >= total_expected {
                return Err(Error::Refused(format!(
                    "{filename}:{line_no}: file contains more than N³ ({total_expected}) entries"
                )));
            }
            entries.push([r, g, b]);
        }

        let Some(n) = size else {
            return Err(Error::Refused(format!(
                "{filename}: missing LUT_3D_SIZE header"
            )));
        };

        let entries = data.unwrap();
        let total_expected = n * n * n;
        if entries.len() != total_expected {
            return Err(Error::Refused(format!(
                "{filename}:{line_no}: expected {total_expected} entries for size {n}, but found {}",
                entries.len()
            )));
        }

        for c in 0..3 {
            if domain_min[c] >= domain_max[c] {
                return Err(Error::Refused(format!(
                    "{filename}: DOMAIN_MIN[{c}] ({}) >= DOMAIN_MAX[{c}] ({})",
                    domain_min[c], domain_max[c]
                )));
            }
        }

        Ok(Self {
            title,
            size: n,
            domain_min,
            domain_max,
            sha256: compute_sha256(bytes),
            data: entries,
        })
    }

    /// Parses an Autodesk / Discreet `.3dl` LUT according to the Autodesk Flame / Lustre specification.
    ///
    /// In Autodesk `.3dl`, the data is stored in **Blue-fastest** axis order:
    /// outer loop is Red, middle loop is Green, inner loop is Blue (`r * N² + g * N + b`).
    /// The values are integers scaled by the declared bit-depth max level.
    ///
    /// When no output scale or bit depth is explicitly declared (via `3DMESH` max level or
    /// `Mesh` output depth), the output bit depth is inferred from the peak value encountered
    /// in the data (≤1023 → 10-bit [1023.0], ≤4095 → 12-bit [4095.0], else 16-bit [65535.0]).
    /// That matches what OpenColorIO (OCIO) does, which is followed here for industry compatibility.
    /// Note that this heuristic has a known edge case: it misreads a dark 12-bit LUT whose largest
    /// value happens to be ≤1023 as a 10-bit LUT.
    pub fn from_3dl(filename: &str, bytes: &[u8]) -> Result<Self, Error> {
        let text = std::str::from_utf8(bytes)
            .map_err(|_| Error::Refused(format!("{filename}: file is not valid UTF-8 text")))?;

        let mut explicit_mesh_size: Option<usize> = None;
        let mut mesh_in_arg: Option<usize> = None;
        let mut declared_max_val: Option<f32> = None;
        let mut data_entries: Vec<[f32; 3]> = Vec::new();
        let mut line_no = 0usize;

        for line in text.lines() {
            line_no += 1;
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            if trimmed.starts_with('<') {
                return Err(Error::Refused(format!(
                    "{filename}:{line_no}: XML is not supported in .3dl files"
                )));
            }

            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.is_empty() {
                continue;
            }

            if parts[0].eq_ignore_ascii_case("3dmesh") {
                if parts.len() == 1 {
                    continue;
                }
                if parts.len() == 3 {
                    let s: usize = parts[1].parse().map_err(|_| {
                        Error::Refused(format!("{filename}:{line_no}: invalid 3DMESH size"))
                    })?;
                    if !(MIN_LUT_SIZE..=MAX_LUT_SIZE).contains(&s) {
                        return Err(Error::Refused(format!(
                            "{filename}:{line_no}: 3DL grid size {s} is outside supported range [{MIN_LUT_SIZE}, {MAX_LUT_SIZE}]"
                        )));
                    }
                    explicit_mesh_size = Some(s);
                    let m: f32 = parts[2].parse().map_err(|_| {
                        Error::Refused(format!("{filename}:{line_no}: invalid 3DMESH max level"))
                    })?;
                    declared_max_val = Some(m);
                    continue;
                }
            }

            if parts[0].eq_ignore_ascii_case("mesh") {
                if parts.len() != 3 {
                    return Err(Error::Refused(format!(
                        "{filename}:{line_no}: Mesh keyword requires 2 arguments (input_depth output_depth)"
                    )));
                }
                let in_val: usize = parts[1].parse().map_err(|_| {
                    Error::Refused(format!(
                        "{filename}:{line_no}: invalid Mesh input argument '{}'",
                        parts[1]
                    ))
                })?;
                if in_val > MAX_LUT_SIZE {
                    return Err(Error::Refused(format!(
                        "{filename}:{line_no}: 3DL grid size {in_val} is outside supported range [{MIN_LUT_SIZE}, {MAX_LUT_SIZE}]"
                    )));
                }
                mesh_in_arg = Some(in_val);

                let out_val: f32 = parts[2].parse().map_err(|_| {
                    Error::Refused(format!(
                        "{filename}:{line_no}: invalid Mesh output argument '{}'",
                        parts[2]
                    ))
                })?;
                let max_level = if out_val <= 16.0 {
                    ((1u32 << (out_val as u32)) - 1) as f32
                } else {
                    out_val
                };
                declared_max_val = Some(max_level);
                continue;
            }

            if parts.len() > 3 {
                // Shaper / mesh points line (e.g. 0 64 128 ... 1023)
                let mut vals = Vec::with_capacity(parts.len());
                for p in &parts {
                    let v: i64 = p.parse().map_err(|_| {
                        Error::Refused(format!(
                            "{filename}:{line_no}: invalid integer in shaper line: '{p}'"
                        ))
                    })?;
                    vals.push(v);
                }
                if explicit_mesh_size.is_none() && mesh_in_arg.is_none() {
                    let s = vals.len();
                    if !(MIN_LUT_SIZE..=MAX_LUT_SIZE).contains(&s) {
                        return Err(Error::Refused(format!(
                            "{filename}:{line_no}: 3DL grid size {s} from shaper is outside supported range [{MIN_LUT_SIZE}, {MAX_LUT_SIZE}]"
                        )));
                    }
                    explicit_mesh_size = Some(s);
                }
                if declared_max_val.is_none() {
                    if let Some(&max_v) = vals.iter().max() {
                        if max_v > 0 {
                            declared_max_val = Some(max_v as f32);
                        }
                    }
                }
                continue;
            }

            if parts.len() == 3 {
                let r: i64 = parts[0].parse().map_err(|_| {
                    Error::Refused(format!(
                        "{filename}:{line_no}: invalid integer in data line: '{}'",
                        parts[0]
                    ))
                })?;
                let g: i64 = parts[1].parse().map_err(|_| {
                    Error::Refused(format!(
                        "{filename}:{line_no}: invalid integer in data line: '{}'",
                        parts[1]
                    ))
                })?;
                let b: i64 = parts[2].parse().map_err(|_| {
                    Error::Refused(format!(
                        "{filename}:{line_no}: invalid integer in data line: '{}'",
                        parts[2]
                    ))
                })?;

                data_entries.push([r as f32, g as f32, b as f32]);
                continue;
            }

            return Err(Error::Refused(format!(
                "{filename}:{line_no}: unexpected line with {} values (expected 3 for RGB data)",
                parts.len()
            )));
        }

        if data_entries.is_empty() {
            return Err(Error::Refused(format!(
                "{filename}: file contains no 3D LUT data entries"
            )));
        }

        let n = if let Some(s) = explicit_mesh_size {
            s
        } else if let Some(in_val) = mesh_in_arg {
            let total = data_entries.len();
            if in_val * in_val * in_val == total {
                in_val
            } else if in_val <= 8 {
                (1usize << in_val) + 1
            } else {
                in_val
            }
        } else {
            let total = data_entries.len();
            let s = (total as f64).cbrt().round() as usize;
            if s * s * s != total {
                return Err(Error::Refused(format!(
                    "{filename}:{line_no}: entry count {total} is not a perfect cube and no mesh size was declared"
                )));
            }
            s
        };

        if !(MIN_LUT_SIZE..=MAX_LUT_SIZE).contains(&n) {
            return Err(Error::Refused(format!(
                "{filename}:{line_no}: 3DL grid size {n} is outside supported range [{MIN_LUT_SIZE}, {MAX_LUT_SIZE}]"
            )));
        }

        let total_expected = n * n * n;
        if data_entries.len() != total_expected {
            return Err(Error::Refused(format!(
                "{filename}:{line_no}: expected {total_expected} entries for size {n}, but found {}",
                data_entries.len()
            )));
        }

        // Output scale heuristic following OpenColorIO (OCIO):
        // Infer bit depth from peak value (<=1023 -> 10-bit, <=4095 -> 12-bit, else 16-bit).
        // Note: As documented on from_3dl, this misreads a dark 12-bit LUT (peak <= 1023) as 10-bit.
        let max_val = if let Some(m) = declared_max_val {
            m
        } else {
            let mut peak = 0.0f32;
            for entry in &data_entries {
                peak = peak.max(entry[0]).max(entry[1]).max(entry[2]);
            }
            if peak > 4095.0 {
                65535.0
            } else if peak > 1023.0 {
                4095.0
            } else if peak > 255.0 {
                1023.0
            } else {
                255.0
            }
        };

        if max_val <= 0.0 {
            return Err(Error::Refused(format!(
                "{filename}: invalid max scale {max_val}"
            )));
        }

        // Reorder from Autodesk 3DL Blue-fastest order (outer R, middle G, inner B)
        // to canonical Red-fastest order (index = r + g * n + b * n * n).
        let mut canonical_data = vec![[0.0f32, 0.0f32, 0.0f32]; total_expected];
        for (k, entry) in data_entries.into_iter().enumerate() {
            let r_idx = k / (n * n);
            let g_idx = (k / n) % n;
            let b_idx = k % n;
            let canonical_idx = r_idx + g_idx * n + b_idx * n * n;
            canonical_data[canonical_idx] =
                [entry[0] / max_val, entry[1] / max_val, entry[2] / max_val];
        }

        Ok(Self {
            title: None,
            size: n,
            domain_min: [0.0, 0.0, 0.0],
            domain_max: [1.0, 1.0, 1.0],
            sha256: compute_sha256(bytes),
            data: canonical_data,
        })
    }

    /// Parses a HALD CLUT image according to the HALD specification (Quel Solaar / ImageMagick).
    ///
    /// The image must be square of dimensions L³ × L³ pixels for integer level L,
    /// defining a 3D cube lattice of size N = L². Both 8-bit and 16-bit PNGs are supported.
    /// In HALD scanline order, Red varies fastest, then Green, then Blue.
    pub fn from_hald_png(filename: &str, bytes: &[u8]) -> Result<Self, Error> {
        let dyn_img = image::load_from_memory(bytes).map_err(|e| {
            Error::Refused(format!("{filename}: failed to decode HALD PNG image: {e}"))
        })?;

        let (w, h) = (dyn_img.width(), dyn_img.height());
        if w != h {
            return Err(Error::Refused(format!(
                "{filename}: HALD CLUT must be square, but image is {w}x{h}"
            )));
        }

        let l = (w as f64).cbrt().round() as usize;
        if l * l * l != w as usize || l < 1 {
            return Err(Error::Refused(format!(
                "{filename}: HALD CLUT dimension {w} is not a valid cube of integer level L (L³)"
            )));
        }

        let n = l * l;
        if !(MIN_LUT_SIZE..=MAX_LUT_SIZE).contains(&n) {
            return Err(Error::Refused(format!(
                "{filename}: HALD CLUT cube size {n} (level {l}) is outside supported range [{MIN_LUT_SIZE}, {MAX_LUT_SIZE}]"
            )));
        }

        let total_expected = n * n * n;
        let mut data = Vec::with_capacity(total_expected);

        // HALD row-major scanline order is Red fastest, Green middle, Blue slowest,
        // which matches canonical order `r + g * n + b * n * n`.
        match &dyn_img {
            image::DynamicImage::ImageRgb16(img) => {
                for px in img.pixels() {
                    data.push([
                        px[0] as f32 / 65535.0,
                        px[1] as f32 / 65535.0,
                        px[2] as f32 / 65535.0,
                    ]);
                }
            }
            image::DynamicImage::ImageRgba16(img) => {
                for px in img.pixels() {
                    data.push([
                        px[0] as f32 / 65535.0,
                        px[1] as f32 / 65535.0,
                        px[2] as f32 / 65535.0,
                    ]);
                }
            }
            _ => {
                let rgb8 = dyn_img.to_rgb8();
                for px in rgb8.pixels() {
                    data.push([
                        px[0] as f32 / 255.0,
                        px[1] as f32 / 255.0,
                        px[2] as f32 / 255.0,
                    ]);
                }
            }
        }

        if data.len() != total_expected {
            return Err(Error::Refused(format!(
                "{filename}: expected {total_expected} pixels, extracted {}",
                data.len()
            )));
        }

        Ok(Self {
            title: None,
            size: n,
            domain_min: [0.0, 0.0, 0.0],
            domain_max: [1.0, 1.0, 1.0],
            sha256: compute_sha256(bytes),
            data,
        })
    }

    /// Evaluates the 3D LUT at the given input RGB coordinate using tetrahedral interpolation.
    ///
    /// 1. Clamps coordinates to `[domain_min, domain_max]` per channel.
    /// 2. Maps through domain boundaries to continuous lattice coordinates in `[0, N-1]`.
    /// 3. Performs tetrahedral interpolation across the 6 simplex partitions.
    #[inline]
    pub fn sample(&self, rgb: [f32; 3]) -> [f32; 3] {
        let n = self.size;
        let n_minus_1 = (n - 1) as f32;

        // 1. Clamp to domain
        let r_clamped = rgb[0].clamp(self.domain_min[0], self.domain_max[0]);
        let g_clamped = rgb[1].clamp(self.domain_min[1], self.domain_max[1]);
        let b_clamped = rgb[2].clamp(self.domain_min[2], self.domain_max[2]);

        // 2. Map through domain to continuous lattice coordinates [0, N-1]
        let r_span = self.domain_max[0] - self.domain_min[0];
        let g_span = self.domain_max[1] - self.domain_min[1];
        let b_span = self.domain_max[2] - self.domain_min[2];

        let u = if r_span > 1e-7 {
            ((r_clamped - self.domain_min[0]) / r_span * n_minus_1).clamp(0.0, n_minus_1)
        } else {
            0.0
        };
        let v = if g_span > 1e-7 {
            ((g_clamped - self.domain_min[1]) / g_span * n_minus_1).clamp(0.0, n_minus_1)
        } else {
            0.0
        };
        let w = if b_span > 1e-7 {
            ((b_clamped - self.domain_min[2]) / b_span * n_minus_1).clamp(0.0, n_minus_1)
        } else {
            0.0
        };

        // 3. Base lattice index and fractional offsets
        let max_idx = n - 2;
        let i = (u.floor() as usize).min(max_idx);
        let j = (v.floor() as usize).min(max_idx);
        let k = (w.floor() as usize).min(max_idx);

        let dr = (u - i as f32).clamp(0.0, 1.0);
        let dg = (v - j as f32).clamp(0.0, 1.0);
        let db = (w - k as f32).clamp(0.0, 1.0);

        let node = |di: usize, dj: usize, dk: usize| -> [f32; 3] {
            let idx = (i + di) + (j + dj) * n + (k + dk) * n * n;
            self.data[idx]
        };

        let p000 = node(0, 0, 0);
        let p111 = node(1, 1, 1);

        // Tetrahedral interpolation over 6 simplices
        if dr >= dg && dg >= db {
            // Simplex 1: dr >= dg >= db
            let p100 = node(1, 0, 0);
            let p110 = node(1, 1, 0);
            [
                p000[0]
                    + dr * (p100[0] - p000[0])
                    + dg * (p110[0] - p100[0])
                    + db * (p111[0] - p110[0]),
                p000[1]
                    + dr * (p100[1] - p000[1])
                    + dg * (p110[1] - p100[1])
                    + db * (p111[1] - p110[1]),
                p000[2]
                    + dr * (p100[2] - p000[2])
                    + dg * (p110[2] - p100[2])
                    + db * (p111[2] - p110[2]),
            ]
        } else if dr >= db && db > dg {
            // Simplex 2: dr >= db > dg
            let p100 = node(1, 0, 0);
            let p101 = node(1, 0, 1);
            [
                p000[0]
                    + dr * (p100[0] - p000[0])
                    + db * (p101[0] - p100[0])
                    + dg * (p111[0] - p101[0]),
                p000[1]
                    + dr * (p100[1] - p000[1])
                    + db * (p101[1] - p100[1])
                    + dg * (p111[1] - p101[1]),
                p000[2]
                    + dr * (p100[2] - p000[2])
                    + db * (p101[2] - p100[2])
                    + dg * (p111[2] - p101[2]),
            ]
        } else if dg > dr && dr >= db {
            // Simplex 3: dg > dr >= db
            let p010 = node(0, 1, 0);
            let p110 = node(1, 1, 0);
            [
                p000[0]
                    + dg * (p010[0] - p000[0])
                    + dr * (p110[0] - p010[0])
                    + db * (p111[0] - p110[0]),
                p000[1]
                    + dg * (p010[1] - p000[1])
                    + dr * (p110[1] - p010[1])
                    + db * (p111[1] - p110[1]),
                p000[2]
                    + dg * (p010[2] - p000[2])
                    + dr * (p110[2] - p010[2])
                    + db * (p111[2] - p110[2]),
            ]
        } else if dg >= db && db > dr {
            // Simplex 4: dg >= db > dr
            let p010 = node(0, 1, 0);
            let p011 = node(0, 1, 1);
            [
                p000[0]
                    + dg * (p010[0] - p000[0])
                    + db * (p011[0] - p010[0])
                    + dr * (p111[0] - p011[0]),
                p000[1]
                    + dg * (p010[1] - p000[1])
                    + db * (p011[1] - p010[1])
                    + dr * (p111[1] - p011[1]),
                p000[2]
                    + dg * (p010[2] - p000[2])
                    + db * (p011[2] - p010[2])
                    + dr * (p111[2] - p011[2]),
            ]
        } else if db > dr && dr >= dg {
            // Simplex 5: db > dr >= dg
            let p001 = node(0, 0, 1);
            let p101 = node(1, 0, 1);
            [
                p000[0]
                    + db * (p001[0] - p000[0])
                    + dr * (p101[0] - p001[0])
                    + dg * (p111[0] - p101[0]),
                p000[1]
                    + db * (p001[1] - p000[1])
                    + dr * (p101[1] - p001[1])
                    + dg * (p111[1] - p101[1]),
                p000[2]
                    + db * (p001[2] - p000[2])
                    + dr * (p101[2] - p001[2])
                    + dg * (p111[2] - p101[2]),
            ]
        } else {
            // Simplex 6: db > dg > dr
            let p001 = node(0, 0, 1);
            let p011 = node(0, 1, 1);
            [
                p000[0]
                    + db * (p001[0] - p000[0])
                    + dg * (p011[0] - p001[0])
                    + dr * (p111[0] - p011[0]),
                p000[1]
                    + db * (p001[1] - p000[1])
                    + dg * (p011[1] - p001[1])
                    + dr * (p111[1] - p011[1]),
                p000[2]
                    + db * (p001[2] - p000[2])
                    + dg * (p011[2] - p001[2])
                    + dr * (p111[2] - p011[2]),
            ]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Generates an Adobe .cube text representation for an identity LUT of size N.
    fn make_identity_cube(n: usize) -> String {
        let mut s = format!("TITLE \"Identity {n}\"\nLUT_3D_SIZE {n}\nDOMAIN_MIN 0.0 0.0 0.0\nDOMAIN_MAX 1.0 1.0 1.0\n");
        let step = 1.0 / (n - 1) as f32;
        // In .cube, red varies fastest, then green, then blue
        for b in 0..n {
            for g in 0..n {
                for r in 0..n {
                    s.push_str(&format!(
                        "{:.6} {:.6} {:.6}\n",
                        r as f32 * step,
                        g as f32 * step,
                        b as f32 * step
                    ));
                }
            }
        }
        s
    }

    #[test]
    fn an_identity_cube_returns_exact_input_values() {
        // Exercise an identity cube of size 17
        let cube_str = make_identity_cube(17);
        let lut = Lut::from_cube("identity17.cube", cube_str.as_bytes()).unwrap();

        // 1. Every 8-bit code on each channel independently
        for c in 0..=255u8 {
            let norm = c as f32 / 255.0;

            // Red channel ramp
            let out_r = lut.sample([norm, 0.0, 0.0]);
            let q_r = (out_r[0] * 255.0).round() as u8;
            let q_g = (out_r[1] * 255.0).round() as u8;
            let q_b = (out_r[2] * 255.0).round() as u8;
            assert_eq!(
                (q_r, q_g, q_b),
                (c, 0, 0),
                "mismatch on red ramp at code {c}"
            );

            // Green channel ramp
            let out_g = lut.sample([0.0, norm, 0.0]);
            let q_r = (out_g[0] * 255.0).round() as u8;
            let q_g = (out_g[1] * 255.0).round() as u8;
            let q_b = (out_g[2] * 255.0).round() as u8;
            assert_eq!(
                (q_r, q_g, q_b),
                (0, c, 0),
                "mismatch on green ramp at code {c}"
            );

            // Blue channel ramp
            let out_b = lut.sample([0.0, 0.0, norm]);
            let q_r = (out_b[0] * 255.0).round() as u8;
            let q_g = (out_b[1] * 255.0).round() as u8;
            let q_b = (out_b[2] * 255.0).round() as u8;
            assert_eq!(
                (q_r, q_g, q_b),
                (0, 0, c),
                "mismatch on blue ramp at code {c}"
            );
        }

        // 2. Colours where R, G and B all differ (not only greys)
        for r_code in (0..=255u8).step_by(17) {
            for g_code in (0..=255u8).step_by(23) {
                for b_code in (0..=255u8).step_by(29) {
                    if r_code == g_code || g_code == b_code || r_code == b_code {
                        continue;
                    }
                    let sampled = lut.sample([
                        r_code as f32 / 255.0,
                        g_code as f32 / 255.0,
                        b_code as f32 / 255.0,
                    ]);
                    let qr = (sampled[0] * 255.0).round() as u8;
                    let qg = (sampled[1] * 255.0).round() as u8;
                    let qb = (sampled[2] * 255.0).round() as u8;
                    assert_eq!(
                        (qr, qg, qb),
                        (r_code, g_code, b_code),
                        "mismatch on distinct RGB colour ({r_code}, {g_code}, {b_code})"
                    );
                }
            }
        }

        // 3. 16-bit ramp: test every 16-bit code per channel (3 x 65,536 samples)
        for c in 0..=65535u16 {
            let norm = c as f32 / 65535.0;

            let out_r = lut.sample([norm, 0.0, 0.0]);
            let qr = (out_r[0] * 65535.0).round() as u16;
            assert_eq!(qr, c, "16-bit red ramp mismatch at {c}");

            let out_g = lut.sample([0.0, norm, 0.0]);
            let qg = (out_g[1] * 65535.0).round() as u16;
            assert_eq!(qg, c, "16-bit green ramp mismatch at {c}");

            let out_b = lut.sample([0.0, 0.0, norm]);
            let qb = (out_b[2] * 65535.0).round() as u16;
            assert_eq!(qb, c, "16-bit blue ramp mismatch at {c}");
        }
    }

    #[test]
    fn a_cube_that_swaps_red_and_blue_swaps_them() {
        // --- 1. Adobe .cube format ---
        // N = 2. Red varies fastest.
        // For input (r, g, b), output is (b, g, r).
        let mut cube_str = String::from("TITLE \"Swap RB\"\nLUT_3D_SIZE 2\n");
        for b in 0..2 {
            for g in 0..2 {
                for r in 0..2 {
                    // Out: (b, g, r)
                    cube_str.push_str(&format!("{b}.0 {g}.0 {r}.0\n"));
                }
            }
        }
        let cube_lut = Lut::from_cube("swap_rb.cube", cube_str.as_bytes()).unwrap();

        // Test pure red -> pure blue
        let out_red = cube_lut.sample([1.0, 0.0, 0.0]);
        assert_eq!(
            out_red,
            [0.0, 0.0, 1.0],
            "Cube pure red must map to pure blue"
        );
        // Test pure blue -> pure red
        let out_blue = cube_lut.sample([0.0, 0.0, 1.0]);
        assert_eq!(
            out_blue,
            [1.0, 0.0, 0.0],
            "Cube pure blue must map to pure red"
        );
        // Test mixed colour
        let out_mix = cube_lut.sample([0.2, 0.4, 0.8]);
        assert!(
            (out_mix[0] - 0.8).abs() < 1e-5,
            "R swapped mismatch: {}",
            out_mix[0]
        );
        assert!(
            (out_mix[1] - 0.4).abs() < 1e-5,
            "G unchanged mismatch: {}",
            out_mix[1]
        );
        assert!(
            (out_mix[2] - 0.2).abs() < 1e-5,
            "B swapped mismatch: {}",
            out_mix[2]
        );

        // --- 2. Autodesk .3dl format ---
        // N = 2, 10-bit declared (max 1023). Blue varies fastest!
        // Outer loop: r in 0..2. Middle loop: g in 0..2. Inner loop: b in 0..2.
        // For entry (r, g, b), output is (b, g, r).
        let mut d3_str = String::from("3DMESH\nMesh 2 1023\n");
        for r in 0..2 {
            for g in 0..2 {
                for b in 0..2 {
                    // Out: (b * 1023, g * 1023, r * 1023)
                    d3_str.push_str(&format!("{} {} {}\n", b * 1023, g * 1023, r * 1023));
                }
            }
        }
        let d3_lut = Lut::from_3dl("swap_rb.3dl", d3_str.as_bytes()).unwrap();

        let out_red = d3_lut.sample([1.0, 0.0, 0.0]);
        assert_eq!(
            out_red,
            [0.0, 0.0, 1.0],
            ".3dl pure red must map to pure blue"
        );
        let out_blue = d3_lut.sample([0.0, 0.0, 1.0]);
        assert_eq!(
            out_blue,
            [1.0, 0.0, 0.0],
            ".3dl pure blue must map to pure red"
        );
        let out_mix = d3_lut.sample([0.2, 0.4, 0.8]);
        assert!(
            (out_mix[0] - 0.8).abs() < 1e-5,
            ".3dl R swapped mismatch: {}",
            out_mix[0]
        );
        assert!(
            (out_mix[1] - 0.4).abs() < 1e-5,
            ".3dl G unchanged mismatch: {}",
            out_mix[1]
        );
        assert!(
            (out_mix[2] - 0.2).abs() < 1e-5,
            ".3dl B swapped mismatch: {}",
            out_mix[2]
        );

        // --- 3. Square HALD .png format ---
        // Level L = 2 -> image dimension W = L³ = 8. Lattice size N = L² = 4.
        // Total pixels = 8 × 8 = 64 = N³.
        // Scanline order: Red varies fastest, then Green, then Blue.
        let l = 2usize;
        let w = l * l * l; // 8
        let n = l * l; // 4
        let mut hald_img = image::RgbImage::new(w as u32, w as u32);
        for p in 0..(n * n * n) {
            let r_idx = p % n;
            let g_idx = (p / n) % n;
            let b_idx = p / (n * n);

            // Swap R and B: output (b, g, r)
            let out_r = (b_idx as f32 / (n - 1) as f32 * 255.0).round() as u8;
            let out_g = (g_idx as f32 / (n - 1) as f32 * 255.0).round() as u8;
            let out_b = (r_idx as f32 / (n - 1) as f32 * 255.0).round() as u8;

            let x = (p % w) as u32;
            let y = (p / w) as u32;
            hald_img.put_pixel(x, y, image::Rgb([out_r, out_g, out_b]));
        }

        let mut png_bytes = Vec::new();
        let mut cursor = std::io::Cursor::new(&mut png_bytes);
        hald_img
            .write_to(&mut cursor, image::ImageFormat::Png)
            .unwrap();

        let hald_lut = Lut::from_hald_png("swap_rb.png", &png_bytes).unwrap();
        let out_red = hald_lut.sample([1.0, 0.0, 0.0]);
        assert_eq!(
            out_red,
            [0.0, 0.0, 1.0],
            "HALD pure red must map to pure blue"
        );
        let out_blue = hald_lut.sample([0.0, 0.0, 1.0]);
        assert_eq!(
            out_blue,
            [1.0, 0.0, 0.0],
            "HALD pure blue must map to pure red"
        );
        let out_mix = hald_lut.sample([0.2, 0.4, 0.8]);
        assert!(
            (out_mix[0] - 0.8).abs() < 1e-2,
            "HALD R swapped mismatch: {}",
            out_mix[0]
        );
        assert!(
            (out_mix[1] - 0.4).abs() < 1e-2,
            "HALD G unchanged mismatch: {}",
            out_mix[1]
        );
        assert!(
            (out_mix[2] - 0.2).abs() < 1e-2,
            "HALD B swapped mismatch: {}",
            out_mix[2]
        );
    }

    #[test]
    fn a_cube_with_the_wrong_number_of_entries_is_refused_with_its_line() {
        // Size 2 requires 2³ = 8 entries. Provide only 5 entries.
        let incomplete_cube = "TITLE \"Incomplete\"\nLUT_3D_SIZE 2\n0.0 0.0 0.0\n0.5 0.0 0.0\n1.0 0.0 0.0\n0.0 0.5 0.0\n0.5 0.5 0.0\n";
        let err = Lut::from_cube("short.cube", incomplete_cube.as_bytes()).unwrap_err();
        match err {
            Error::Refused(msg) => {
                assert!(
                    msg.contains("short.cube"),
                    "expected filename in error, got: {msg}"
                );
                assert!(
                    msg.contains(":7:"),
                    "expected line number 7 in error, got: {msg}"
                );
                assert!(
                    msg.contains("expected 8 entries"),
                    "expected entry count in error, got: {msg}"
                );
            }
            other => panic!("expected Error::Refused, got {other:?}"),
        }

        // Test extra entry beyond N³
        let overflow_cube = "LUT_3D_SIZE 2\n0.0 0.0 0.0\n1.0 0.0 0.0\n0.0 1.0 0.0\n1.0 1.0 0.0\n0.0 0.0 1.0\n1.0 0.0 1.0\n0.0 1.0 1.0\n1.0 1.0 1.0\n0.5 0.5 0.5\n";
        let err = Lut::from_cube("overflow.cube", overflow_cube.as_bytes()).unwrap_err();
        match err {
            Error::Refused(msg) => {
                assert!(
                    msg.contains("overflow.cube"),
                    "expected filename in error, got: {msg}"
                );
                assert!(
                    msg.contains(":10:"),
                    "expected line 10 in error, got: {msg}"
                );
                assert!(
                    msg.contains("more than N³"),
                    "expected overflow message, got: {msg}"
                );
            }
            other => panic!("expected Error::Refused, got {other:?}"),
        }
    }

    #[test]
    fn a_lut_claiming_an_enormous_size_is_refused_before_allocating() {
        let huge_cube = "LUT_3D_SIZE 4096\n0.0 0.0 0.0\n";
        let err = Lut::from_cube("huge.cube", huge_cube.as_bytes()).unwrap_err();
        match err {
            Error::Refused(msg) => {
                assert!(msg.contains("huge.cube"), "expected filename, got: {msg}");
                assert!(
                    msg.contains("4096"),
                    "expected size 4096 mentioned, got: {msg}"
                );
                assert!(
                    msg.contains("outside supported range"),
                    "expected range mention, got: {msg}"
                );
            }
            other => panic!("expected Error::Refused, got {other:?}"),
        }
    }

    #[test]
    fn a_1d_cube_is_refused_clearly() {
        let one_d_cube = "LUT_1D_SIZE 256\n0.0 0.0 0.0\n";
        let err = Lut::from_cube("curve.cube", one_d_cube.as_bytes()).unwrap_err();
        match err {
            Error::Refused(msg) => {
                assert!(msg.contains("curve.cube"), "expected filename, got: {msg}");
                assert!(
                    msg.contains("1D LUTs are not supported"),
                    "expected clear 1D rejection, got: {msg}"
                );
                assert!(
                    msg.contains("LUT_1D_SIZE"),
                    "expected LUT_1D_SIZE mentioned, got: {msg}"
                );
            }
            other => panic!("expected Error::Refused, got {other:?}"),
        }
    }

    #[test]
    fn values_outside_the_domain_are_clamped_before_lookup() {
        // LUT with custom domain [0.2, 0.8] per channel
        let cube_str = "TITLE \"Subdomain\"\nLUT_3D_SIZE 2\nDOMAIN_MIN 0.2 0.2 0.2\nDOMAIN_MAX 0.8 0.8 0.8\n0.1 0.1 0.1\n0.9 0.1 0.1\n0.1 0.9 0.1\n0.9 0.9 0.1\n0.1 0.1 0.9\n0.9 0.1 0.9\n0.1 0.9 0.9\n0.9 0.9 0.9\n";
        let lut = Lut::from_cube("subdomain.cube", cube_str.as_bytes()).unwrap();

        // Sample below domain min (0.0, 0.0, 0.0) -> must clamp to (0.2, 0.2, 0.2), which maps to node (0,0,0) -> [0.1, 0.1, 0.1]
        let out_low = lut.sample([0.0, 0.0, 0.0]);
        assert_eq!(
            out_low,
            [0.1, 0.1, 0.1],
            "values below domain_min must clamp to min node"
        );

        // Sample above domain max (1.0, 1.0, 1.0) -> must clamp to (0.8, 0.8, 0.8), which maps to node (1,1,1) -> [0.9, 0.9, 0.9]
        let out_high = lut.sample([1.0, 1.0, 1.0]);
        assert_eq!(
            out_high,
            [0.9, 0.9, 0.9],
            "values above domain_max must clamp to max node"
        );

        // Sample negative values -> clamp to min node
        let out_neg = lut.sample([-10.0, -5.0, -1.0]);
        assert_eq!(
            out_neg,
            [0.1, 0.1, 0.1],
            "negative values must clamp to min node"
        );
    }
}
