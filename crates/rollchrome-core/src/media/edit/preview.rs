//! Interactive preview rendering and session management in `core` (ED-4).
//!
//! Provides `PreviewSession`, which holds cached pre-linearised proxy buffers
//! for drag (720p) and settle (1440p) stages, enabling responsive 30–60 fps
//! slider scrubbing without re-decoding or re-linearising full-frame photographs.
//!
//! ### Downscaling Strategy (Integer Space Before Linearisation)
//! Downscaling is performed in decoded integer space (`Rgb8` / `Rgb16`) before linearising:
//! 1. **Memory footprint**: A 36 MP linear `f32` buffer requires ~433 MB RAM. Allocating and
//!    transforming full-frame floating-point buffers creates severe memory spikes and cache
//!    thrashing. Downscaling integer pixels first allocates only ~13 MB (720p) and ~52 MB
//!    (1440p) for the cached linear proxies, reducing peak RAM by >350 MB.
//! 2. **Speed & SIMD efficiency**: `fast_image_resize` provides SIMD-accelerated (NEON/AVX2)
//!    assembly kernels for `U8x3` and `U16x3` integer resizing. Full session creation
//!    (both proxies downscaled and linearised) takes **113 ms** (measured on Apple Silicon
//!    with a 36 MP frame), well below the ≤ 1 s budget.
//! 3. **Linearisation efficiency**: Linearising only the two downscaled proxies (~5.4 MP combined)
//!    via 256- and 65,536-entry decode lookup tables takes ~1 ms, completely eliminating
//!    redundant conversions of discarded full-frame pixels.
//!
//! ### Downscaling Trade-Off
//! Averaging in encoded (gamma) space darkens fine high-contrast detail slightly in the **preview**,
//! relative to the export, which renders at full resolution. That is a fair trade for an interactive
//! preview, and documented so nobody mistakes it for a pipeline error when comparing a preview
//! with an export at 100%.
//!
//! ### Resolution Targets
//! - **Drag stage**: 720p (1280 px long edge, ~1.09 MP for 3:2). Chosen because active scrubbing
//!   demands continuous, stutter-free visual motor continuity at 30–60 fps. Rendering 720p
//!   executes at **8.2 ms p95** (measured on Apple Silicon, 36 MP source, tone adjustments + 33³ LUT),
//!   comfortably below the 25 ms core render budget and leaving ample headroom for IPC transfer
//!   and webview display.
//! - **Settle stage**: 1440p (2560 px long edge, ~4.37 MP for 3:2). Renders at **35.5 ms p95**
//!   (measured on Apple Silicon), restoring full retina sharpness on mouse release within the
//!   60 ms core render budget.

use super::brush::{BrushCache, BrushRaster};
use super::curves::ToneCurvesTable;
use super::lut::Lut;
use super::masks::{CompiledMasks, LocalAdjustments, Mask, MaskKind, MaskRasters};
use super::pipeline::{linear_to_srgb, validate_lut, AdjustmentRecipe, ImageBuffer, LinearBuffer};
use super::raster::{CoverageRaster, StoredRaster};
use crate::media::histogram::{histogram, Histogram};
use fast_image_resize as fr;
use phototools_core::error::Error;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::sync::{LazyLock, Mutex};

/// Size of the fast linear-to-sRGB preview lookup table.
const FAST_SRGB_SIZE: usize = 4096;

/// 4096-entry lookup table mapping linear light float [0.0, 1.0] to display sRGB.
///
/// Uses square-root non-linear parameterisation ($t = \sqrt{l}$), ensuring uniform
/// precision across both dark shadows and bright highlights with continuous derivatives.
/// Combined with linear interpolation, maximum error across [0.0, 1.0] is below
/// 6.5e-7, guaranteeing exact match with analytical `linear_to_srgb` at 8-bit quantization.
pub static FAST_LINEAR_TO_SRGB: LazyLock<Box<[f32; FAST_SRGB_SIZE]>> = LazyLock::new(|| {
    let mut table = vec![0.0f32; FAST_SRGB_SIZE];
    for (i, item) in table.iter_mut().enumerate() {
        let t = i as f32 / (FAST_SRGB_SIZE - 1) as f32;
        let lin = t * t;
        *item = linear_to_srgb(lin);
    }
    table.into_boxed_slice().try_into().unwrap()
});

/// Fast linear-to-sRGB transfer for preview rendering with a direct table reference.
#[inline(always)]
pub fn fast_linear_to_srgb_table(l: f32, table: &[f32; FAST_SRGB_SIZE]) -> f32 {
    if l <= 0.0 {
        return 0.0;
    }
    if l >= 1.0 {
        return 1.0;
    }
    let t = l.sqrt() * (FAST_SRGB_SIZE - 1) as f32;
    let idx = (t as usize).min(FAST_SRGB_SIZE - 2);
    let frac = t - idx as f32;
    unsafe {
        let v0 = *table.get_unchecked(idx);
        let v1 = *table.get_unchecked(idx + 1);
        v0 + (v1 - v0) * frac
    }
}

/// Fast linear-to-sRGB transfer for preview rendering.
#[inline(always)]
pub fn fast_linear_to_srgb(l: f32) -> f32 {
    fast_linear_to_srgb_table(l, &FAST_LINEAR_TO_SRGB)
}

/// Maximum long-edge pixel dimension for active dragging preview (720p long edge).
pub const DRAG_MAX_EDGE: u32 = 1280;

/// Maximum long-edge pixel dimension for settled preview on release (1440p long edge).
pub const SETTLE_MAX_EDGE: u32 = 2560;

/// Maximum long-edge pixel dimension for full-resolution preview.
///
/// Limited to 8192 px because HTML5 Canvas has area and texture size limits across
/// web browsers (e.g. WebKit canvas size limits, WebGL MAX_TEXTURE_SIZE). Photographs
/// with a long edge exceeding 8192 px are scaled to this limit for the Full preview stage.
pub const FULL_MAX_EDGE: u32 = 8192;

/// Stage of interactive preview rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PreviewStage {
    /// 720p long-edge proxy during active slider dragging.
    Drag,
    /// 1440p long-edge proxy when mouse is released to settle.
    Settle,
    /// Full-resolution preview (up to 8192 px on the long edge).
    Full,
}

/// White balance pick result from eyedropper tool (RP-4).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WhiteBalancePick {
    pub temperature: f32,
    pub tint: f32,
    pub clamped: bool,
}

/// Auto tone calculation result (RP-4).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AutoTone {
    pub exposure: f32,
    pub whites: f32,
    pub blacks: f32,
}

/// An uncompressed RGBA8 pixel frame ready for HTML5 Canvas ImageData or display.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RgbaFrame {
    pub width: u32,
    pub height: u32,
    pub bytes: Vec<u8>,
    pub orientation: u32,
    /// Counted from `bytes`, the values the person sees, so the histogram and the
    /// clipping warnings describe the frame on screen and never a different one (ED-15).
    pub histogram: Histogram,
    /// This frame's normalised coordinates → the stored frame's (`masks::output_to_stored`),
    /// so the view places mask handles where the masks act (ED-20).
    pub to_stored: [f32; 6],
}

/// Bytes ahead of the pixels in a frame sent to the Edit view: width, height,
/// delivered orientation, histogram pixel count, highlight- and shadow-clipped pixel
/// counts, the six coefficients of `to_stored` (as `f32` bits, ED-20), then 256 red,
/// green, blue and luminance bins. Every field is a big-endian 32-bit word.
///
/// The plan sketched a 1032-byte header, which holds one channel at 32 bits or four
/// at 8; the four channels the view switches between need exact counts, and
/// 4 KiB is nothing beside a settle frame's 17 MiB of pixels.
pub const PREVIEW_FRAME_HEADER_LEN: usize = (6 + 6 + 4 * 256) * 4;

/// Lays a preview frame out for the Edit view (see [`PREVIEW_FRAME_HEADER_LEN`]).
///
/// It lives here rather than in the desktop command because the layout is a
/// contract the view decodes, and a contract belongs where it can be tested
/// without a window (G1).
pub fn encode_preview_frame(frame: &RgbaFrame) -> Vec<u8> {
    let h = &frame.histogram;
    let mut out = Vec::with_capacity(PREVIEW_FRAME_HEADER_LEN + frame.bytes.len());
    for v in [
        frame.width,
        frame.height,
        frame.orientation,
        h.pixels,
        h.highlight_clipped,
        h.shadow_clipped,
    ] {
        out.extend_from_slice(&v.to_be_bytes());
    }
    for c in frame.to_stored {
        out.extend_from_slice(&c.to_bits().to_be_bytes());
    }
    for channel in [&h.red, &h.green, &h.blue, &h.luminance] {
        for v in channel.iter() {
            out.extend_from_slice(&v.to_be_bytes());
        }
    }
    out.extend_from_slice(&frame.bytes);
    out
}

struct CachedGeometry {
    geometry: super::geometry::Geometry,
    detail: Option<super::detail::Detail>,
    drag_transformed: Option<Arc<LinearBuffer>>,
    settle_transformed: Option<Arc<LinearBuffer>>,
    full_transformed: Option<Arc<LinearBuffer>>,
}

type StoredMaskKey = (String, Option<usize>);
type StoredMaskEntry = (u64, Arc<CoverageRaster>);
type LocalDetailKey = (String, PreviewStage);
type LocalDetailEntry = (
    super::detail::Detail,
    Option<super::geometry::Geometry>,
    Arc<LinearBuffer>,
);

/// An interactive preview session constructed once per image.
///
/// Holds pre-downscaled, pre-linearised proxies for drag (720p) and settle (1440p)
/// stages. Linearisation is performed once during session creation rather than per slider tick.
/// The full-resolution linear buffer is decoded and linearised lazily when first requested
/// by `PreviewStage::Full`, capped at `FULL_MAX_EDGE = 8192`.
///
/// In ED-13:
/// Holds cached geometry-transformed linear proxies (`cached_geom`) when geometry adjustments
/// are active, preventing expensive geometry resampling during continuous colour slider drags.
/// Thread-safe `geom_recompute_count` tracks recomputations for performance verification.
pub struct PreviewSession {
    drag_linear: Arc<LinearBuffer>,
    settle_linear: Arc<LinearBuffer>,
    source_path: Option<std::path::PathBuf>,
    source_clone: Option<ImageBuffer>,
    full_linear: Mutex<Option<Arc<LinearBuffer>>>,
    full_decodes: AtomicUsize,
    full_dimensions: (u32, u32),
    pub orientation: u32,
    pub source_sha256: String,
    pub source_long_edge: u32,
    cached_geom: Mutex<Option<CachedGeometry>>,
    pub geom_recompute_count: AtomicUsize,
    cached_detail_drag: Mutex<Option<(super::detail::Detail, Arc<LinearBuffer>)>>,
    cached_detail_settle: Mutex<Option<(super::detail::Detail, Arc<LinearBuffer>)>>,
    cached_detail_full: Mutex<Option<(super::detail::Detail, Arc<LinearBuffer>)>>,
    pub detail_recompute_count: AtomicUsize,
    /// Brush rasters per mask and stage, so painting redraws only the new segments (ED-21).
    brushes: Mutex<HashMap<(String, PreviewStage), BrushCache>>,
    /// Decoded automatic masks per (mask, component), with a hash of the PNG they came from (ED-23, RP-11).
    stored: Mutex<HashMap<StoredMaskKey, StoredMaskEntry>>,
    /// Cached local detail frames per (mask, stage) with combined Detail and geometry (RP-11b).
    local_detail_cache: Mutex<HashMap<LocalDetailKey, LocalDetailEntry>>,
}

impl PreviewSession {
    /// Number of auto mask rasters currently held in the session's raster cache (RP-11).
    pub fn cached_raster_count(&self) -> usize {
        self.stored.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    /// Number of local detail frames currently held in the session's cache (RP-11b).
    pub fn cached_local_detail_count(&self) -> usize {
        self.local_detail_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .len()
    }
}

/// A session's caches, offered to mask compilation for one stage.
struct SessionRasters<'a> {
    session: &'a PreviewSession,
    stage: PreviewStage,
    w: u32,
    h: u32,
}

impl MaskRasters for SessionRasters<'_> {
    fn brush(&mut self, m: &Mask) -> Arc<BrushRaster> {
        self.session.brush_raster(m, self.stage, self.w, self.h)
    }

    fn stored(
        &mut self,
        m: &Mask,
        comp_idx: Option<usize>,
        raster: &StoredRaster,
    ) -> Result<Arc<CoverageRaster>, Error> {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        raster.hash(&mut hasher);
        let key = hasher.finish();
        let cache_key = (m.id.clone(), comp_idx);
        let mut cache = self
            .session
            .stored
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some((k, r)) = cache.get(&cache_key) {
            if *k == key {
                return Ok(r.clone());
            }
        }
        let decoded = Arc::new(raster.decode()?);
        cache.insert(cache_key, (key, decoded.clone()));
        Ok(decoded)
    }
}

impl PreviewSession {
    /// Decode an image from disk and construct an interactive preview session with EXIF orientation.
    pub fn open(path: &std::path::Path) -> Result<Self, Error> {
        let bytes = std::fs::read(path)?;
        let source_sha256 = phototools_core::ingest::scanner::hex(&sha2::Sha256::digest(&bytes));
        let orientation = phototools_core::media::meta::read_meta(path)
            .map(|m| m.orientation as u32)
            .unwrap_or(1);
        let image = super::pipeline::decode_image(path)?;
        let full_dimensions = fit_full_edge(image.width(), image.height(), FULL_MAX_EDGE);
        let source_long_edge = image.width().max(image.height()).max(1);
        let drag_buf = downscale_image_buffer(&image, DRAG_MAX_EDGE)?;
        let settle_buf = downscale_image_buffer(&image, SETTLE_MAX_EDGE)?;

        let drag_linear = Arc::new(LinearBuffer::from_image_buffer(&drag_buf));
        let settle_linear = Arc::new(LinearBuffer::from_image_buffer(&settle_buf));

        Ok(Self {
            drag_linear,
            settle_linear,
            source_path: Some(path.to_path_buf()),
            source_clone: None,
            full_linear: Mutex::new(None),
            full_decodes: AtomicUsize::new(0),
            full_dimensions,
            orientation: if orientation == 0 { 1 } else { orientation },
            source_sha256,
            source_long_edge,
            cached_geom: Mutex::new(None),
            geom_recompute_count: AtomicUsize::new(0),
            cached_detail_drag: Mutex::new(None),
            cached_detail_settle: Mutex::new(None),
            cached_detail_full: Mutex::new(None),
            detail_recompute_count: AtomicUsize::new(0),
            brushes: Mutex::new(HashMap::new()),
            stored: Mutex::new(HashMap::new()),
            local_detail_cache: Mutex::new(HashMap::new()),
        })
    }

    /// Build a preview session from a decoded image buffer.
    pub fn new(image: &ImageBuffer) -> Result<Self, Error> {
        let mut session = Self::from_image_buffer_with_orientation(image, 1)?;
        session.source_sha256 = format!("{:08x}{:08x}", image.width(), image.height());
        Ok(session)
    }

    /// Build a preview session from a decoded image buffer.
    pub fn from_image_buffer(image: &ImageBuffer) -> Result<Self, Error> {
        let mut session = Self::from_image_buffer_with_orientation(image, 1)?;
        session.source_sha256 = format!("{:08x}{:08x}", image.width(), image.height());
        Ok(session)
    }

    /// Build a preview session from an image buffer with specified EXIF orientation.
    pub fn from_image_buffer_with_orientation(
        image: &ImageBuffer,
        orientation: u32,
    ) -> Result<Self, Error> {
        let full_dimensions = fit_full_edge(image.width(), image.height(), FULL_MAX_EDGE);
        let source_long_edge = image.width().max(image.height()).max(1);
        let drag_buf = downscale_image_buffer(image, DRAG_MAX_EDGE)?;
        let settle_buf = downscale_image_buffer(image, SETTLE_MAX_EDGE)?;

        let drag_linear = Arc::new(LinearBuffer::from_image_buffer(&drag_buf));
        let settle_linear = Arc::new(LinearBuffer::from_image_buffer(&settle_buf));

        Ok(Self {
            drag_linear,
            settle_linear,
            source_path: None,
            source_clone: Some(image.clone()),
            full_linear: Mutex::new(None),
            full_decodes: AtomicUsize::new(0),
            full_dimensions,
            orientation: if orientation == 0 { 1 } else { orientation },
            source_sha256: String::new(),
            source_long_edge,
            cached_geom: Mutex::new(None),
            geom_recompute_count: AtomicUsize::new(0),
            cached_detail_drag: Mutex::new(None),
            cached_detail_settle: Mutex::new(None),
            cached_detail_full: Mutex::new(None),
            detail_recompute_count: AtomicUsize::new(0),
            brushes: Mutex::new(HashMap::new()),
            stored: Mutex::new(HashMap::new()),
            local_detail_cache: Mutex::new(HashMap::new()),
        })
    }

    /// Fluent builder to override preview orientation.
    pub fn with_orientation(mut self, orientation: u32) -> Self {
        self.orientation = if orientation == 0 { 1 } else { orientation };
        self
    }

    /// Reference to the linearised drag proxy buffer.
    pub fn drag_linear(&self) -> &LinearBuffer {
        &self.drag_linear
    }

    /// Reference to the linearised settle proxy buffer.
    pub fn settle_linear(&self) -> &LinearBuffer {
        &self.settle_linear
    }

    /// Dimensions (width, height) of the drag proxy frame.
    pub fn drag_dimensions(&self) -> (u32, u32) {
        (self.drag_linear.width, self.drag_linear.height)
    }

    /// Dimensions (width, height) of the settle proxy frame.
    pub fn settle_dimensions(&self) -> (u32, u32) {
        (self.settle_linear.width, self.settle_linear.height)
    }

    /// Dimensions (width, height) of the full preview frame, without decoding.
    pub fn full_dimensions(&self) -> (u32, u32) {
        self.full_dimensions
    }

    /// Number of times the full source buffer was decoded or converted.
    pub fn full_decodes(&self) -> usize {
        self.full_decodes.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Number of times detail was computed across all preview stages.
    pub fn detail_computes(&self) -> usize {
        self.detail_recompute_count
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Provides the linear proxy for `stage`, applying and caching `detail` if specified and not identity.
    pub fn stage_linear(
        &self,
        stage: PreviewStage,
        detail: Option<&super::detail::Detail>,
    ) -> Result<Arc<LinearBuffer>, Error> {
        let active_detail = match detail {
            Some(d) if !d.is_identity() => d,
            _ => {
                return match stage {
                    PreviewStage::Drag => Ok(self.drag_linear.clone()),
                    PreviewStage::Settle => Ok(self.settle_linear.clone()),
                    PreviewStage::Full => self.ensure_full_linear(),
                };
            }
        };

        let (cache_mutex, base) = match stage {
            PreviewStage::Drag => (&self.cached_detail_drag, self.drag_linear.clone()),
            PreviewStage::Settle => (&self.cached_detail_settle, self.settle_linear.clone()),
            PreviewStage::Full => {
                let full = self.ensure_full_linear()?;
                (&self.cached_detail_full, full)
            }
        };

        let mut lock = cache_mutex.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((cached_det, cached_buf)) = lock.as_ref() {
            if cached_det == active_detail {
                return Ok(cached_buf.clone());
            }
        }

        self.detail_recompute_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let scale = (base.width.max(base.height) as f32) / (self.source_long_edge as f32);
        let processed = Arc::new(super::detail::apply_detail(&base, active_detail, scale));
        *lock = Some((active_detail.clone(), processed.clone()));
        Ok(processed)
    }

    fn ensure_full_linear(&self) -> Result<Arc<LinearBuffer>, Error> {
        let mut lock = self.full_linear.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(buf) = lock.as_ref() {
            return Ok(buf.clone());
        }
        let image = if let Some(path) = &self.source_path {
            super::pipeline::decode_image(path)?
        } else if let Some(clone) = &self.source_clone {
            clone.clone()
        } else {
            return Err(Error::Internal(
                "No source available for full stage decode".into(),
            ));
        };
        self.full_decodes
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let fitted = downscale_image_buffer(&image, FULL_MAX_EDGE)?;
        let linear = Arc::new(LinearBuffer::from_image_buffer(&fitted));
        *lock = Some(linear.clone());
        Ok(linear)
    }

    fn raw_stage_linear(&self, stage: PreviewStage) -> Result<Arc<LinearBuffer>, Error> {
        match stage {
            PreviewStage::Drag => Ok(self.drag_linear.clone()),
            PreviewStage::Settle => Ok(self.settle_linear.clone()),
            PreviewStage::Full => self.ensure_full_linear(),
        }
    }

    /// Renders an RGBA8 frame for the requested preview stage with the specified recipe and LUT.
    ///
    /// Validates LUT presence and hash equality via `validate_lut`.
    /// Composes geometry into cached upright linear proxies if geometry is active,
    /// delivering an upright frame (orientation = 1) without recomputing geometry during colour drags.
    pub fn render(
        &self,
        recipe: &AdjustmentRecipe,
        lut: Option<&Lut>,
        stage: PreviewStage,
    ) -> Result<RgbaFrame, Error> {
        validate_lut(recipe, lut)?;

        let effective_recipe;
        let recipe_ref = if recipe.source_sha256.is_empty() && !self.source_sha256.is_empty() {
            effective_recipe = {
                let mut r = recipe.clone();
                r.source_sha256 = self.source_sha256.clone();
                r
            };
            &effective_recipe
        } else {
            recipe
        };

        let (proxy, to_stored, frame_orientation, geom_opt) = if let Some(geom) =
            recipe_ref.geometry.as_ref()
        {
            let active_detail = recipe_ref
                .detail
                .as_ref()
                .filter(|d| !d.is_identity())
                .cloned();
            let mut cache = self.cached_geom.lock().unwrap_or_else(|e| e.into_inner());
            let hit = match &*cache {
                Some(c) => c.geometry == *geom && c.detail == active_detail,
                None => false,
            };
            if !hit {
                self.geom_recompute_count
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                *cache = Some(CachedGeometry {
                    geometry: geom.clone(),
                    detail: active_detail,
                    drag_transformed: None,
                    settle_transformed: None,
                    full_transformed: None,
                });
            }
            let c = cache.as_mut().unwrap();
            match stage {
                PreviewStage::Drag => {
                    if c.drag_transformed.is_none() {
                        let input = self.stage_linear(stage, recipe_ref.detail.as_ref())?;
                        c.drag_transformed =
                            Some(Arc::new(geom.apply_to_linear(&input, self.orientation)?));
                    }
                }
                PreviewStage::Settle => {
                    if c.settle_transformed.is_none() {
                        let input = self.stage_linear(stage, recipe_ref.detail.as_ref())?;
                        c.settle_transformed =
                            Some(Arc::new(geom.apply_to_linear(&input, self.orientation)?));
                    }
                }
                PreviewStage::Full => {
                    if c.full_transformed.is_none() {
                        let input = self.stage_linear(stage, recipe_ref.detail.as_ref())?;
                        c.full_transformed =
                            Some(Arc::new(geom.apply_to_linear(&input, self.orientation)?));
                    }
                }
            }
            let proxy = match stage {
                PreviewStage::Drag => c.drag_transformed.clone().unwrap(),
                PreviewStage::Settle => c.settle_transformed.clone().unwrap(),
                PreviewStage::Full => c.full_transformed.clone().unwrap(),
            };
            let (stored_w, stored_h) = self.stored_dims(stage);
            let to_stored =
                super::masks::output_to_stored(Some(geom), stored_w, stored_h, self.orientation)?;
            (proxy, to_stored, 1, Some(geom))
        } else {
            let proxy = self.stage_linear(stage, recipe_ref.detail.as_ref())?;
            (
                proxy,
                [1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
                self.orientation,
                None,
            )
        };

        let masks = self.compile_masks(recipe_ref, geom_opt, stage)?;

        let (frame_buf, base_ref) = if let Some(compiled_masks) =
            masks.as_ref().filter(|m| m.has_local_detail())
        {
            let raw_source = self.raw_stage_linear(stage)?;
            let scale =
                (raw_source.width.max(raw_source.height) as f32) / (self.source_long_edge as f32);

            let mut detail_frames = Vec::new();
            let mut cache = self
                .local_detail_cache
                .lock()
                .unwrap_or_else(|e| e.into_inner());

            for m in compiled_masks.active_masks() {
                if !m.detail.is_zero() {
                    let combined =
                        super::detail::combine_detail(recipe_ref.detail.as_ref(), &m.detail);
                    let geom_opt_cloned = recipe_ref.geometry.clone();
                    let key = (m.id.clone(), stage);

                    let arc_buf = match cache.get(&key) {
                        Some((d, g, buf)) if d == &combined && g == &geom_opt_cloned => buf.clone(),
                        _ => {
                            self.detail_recompute_count
                                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            let d_buf = super::detail::apply_detail(&raw_source, &combined, scale);
                            let d_post = if let Some(geom) = &recipe_ref.geometry {
                                geom.apply_to_linear(&d_buf, self.orientation)?
                            } else {
                                d_buf
                            };
                            let arc = Arc::new(d_post);
                            cache.insert(key, (combined, geom_opt_cloned, arc.clone()));
                            arc
                        }
                    };
                    detail_frames.push((m, arc_buf));
                }
            }
            drop(cache);

            let borrowed_frames: Vec<(&super::masks::CompiledMask, &LinearBuffer)> = detail_frames
                .iter()
                .map(|(m, buf)| (*m, buf.as_ref()))
                .collect();

            let composite =
                super::masks::blend_local_detail(&proxy, &borrowed_frames, compiled_masks);
            (Arc::new(composite), Some(proxy.clone()))
        } else {
            (proxy.clone(), None)
        };

        let mut frame = render_rgba_frame_masked_with_base(
            &frame_buf,
            recipe_ref,
            lut,
            masks.as_ref(),
            base_ref.as_deref(),
        )?;
        frame.orientation = frame_orientation;
        frame.to_stored = to_stored;
        Ok(frame)
    }

    /// The photograph's stored pixels, 8-bit sRGB, from the settle proxy, for an automatic
    /// mask (ED-23). The models see 1024 px at most, so the proxy (2560 on the long edge)
    /// loses them nothing, and the source is not decoded a second time.
    pub fn segmentation_input(&self) -> image::RgbImage {
        let p = &self.settle_linear;
        let bytes: Vec<u8> = p
            .data
            .iter()
            .map(|&v| super::pipeline::linear_to_u8(v))
            .collect();
        image::RgbImage::from_raw(p.width, p.height, bytes)
            .expect("a linear proxy holds three values per pixel")
    }

    fn stored_dims(&self, stage: PreviewStage) -> (u32, u32) {
        match stage {
            PreviewStage::Drag => (self.drag_linear.width, self.drag_linear.height),
            PreviewStage::Settle => (self.settle_linear.width, self.settle_linear.height),
            PreviewStage::Full => self.full_dimensions,
        }
    }

    /// The masks of `recipe` for a frame at `stage`, with brush rasters from the cache.
    fn compile_masks(
        &self,
        recipe: &AdjustmentRecipe,
        geometry: Option<&super::geometry::Geometry>,
        stage: PreviewStage,
    ) -> Result<Option<CompiledMasks>, Error> {
        let (w, h) = self.stored_dims(stage);
        // Forget the rasters of masks that are gone.
        self.brushes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|(id, _), _| recipe.masks.iter().any(|m| &m.id == id));
        self.stored
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|(id, _), _| recipe.masks.iter().any(|m| &m.id == id));
        self.local_detail_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|(id, _), _| recipe.masks.iter().any(|m| &m.id == id));
        CompiledMasks::compile_with(
            &recipe.masks,
            geometry,
            w,
            h,
            self.orientation,
            &mut SessionRasters {
                session: self,
                stage,
                w,
                h,
            },
        )
    }

    /// The brush rasters of mask `m` at `stage`, drawing only what changed since last asked.
    fn brush_raster(&self, m: &Mask, stage: PreviewStage, w: u32, h: u32) -> Arc<BrushRaster> {
        let mut caches = self.brushes.lock().unwrap_or_else(|e| e.into_inner());
        caches
            .entry((m.id.clone(), stage))
            .or_insert_with(|| BrushCache::new(w, h))
            .update(&m.strokes)
    }

    /// Segments the brush caches have drawn, so a test can see that painting a stroke
    /// draws each new segment once (ED-21).
    pub fn brush_segments_drawn(&self) -> usize {
        self.brushes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .map(|c| c.segments_drawn)
            .sum()
    }

    /// Where the mask `mask_id` of `recipe` acts on the frame `render` would return for the
    /// same recipe and stage, as width, height and 8-bit coverage (ED-20).
    pub fn mask_coverage(
        &self,
        recipe: &AdjustmentRecipe,
        mask_id: &str,
        stage: PreviewStage,
    ) -> Result<(u32, u32, Vec<u8>), Error> {
        let mask = recipe
            .masks
            .iter()
            .find(|m| m.id == mask_id)
            .ok_or_else(|| Error::Refused(format!("No mask {mask_id} in this recipe")))?;
        let input = self.stage_linear(stage, recipe.detail.as_ref())?;
        let (stored_w, stored_h) = (input.width, input.height);
        let has_range = matches!(
            mask.kind,
            MaskKind::LuminanceRange { .. } | MaskKind::ColourRange { .. }
        ) || mask.components.iter().any(|c| {
            matches!(
                c.kind,
                MaskKind::LuminanceRange { .. } | MaskKind::ColourRange { .. }
            )
        });
        let transformed_buf;
        let (out_w, out_h, affine, pixels_ref): (u32, u32, [f32; 6], &[f32]) =
            match recipe.geometry.as_ref() {
                Some(g) => {
                    let plan = g.plan(stored_w, stored_h, self.orientation)?;
                    if has_range {
                        transformed_buf = g.apply_to_linear(&input, self.orientation)?;
                        (
                            plan.out_w,
                            plan.out_h,
                            plan.normalised_affine(),
                            &transformed_buf.data,
                        )
                    } else {
                        (
                            plan.out_w,
                            plan.out_h,
                            plan.normalised_affine(),
                            &input.data,
                        )
                    }
                }
                None => (
                    stored_w,
                    stored_h,
                    [1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
                    &input.data,
                ),
            };
        let coverage = super::masks::coverage_frame(
            mask,
            &mut SessionRasters {
                session: self,
                stage,
                w: stored_w,
                h: stored_h,
            },
            affine,
            stored_w,
            stored_h,
            out_w,
            out_h,
            pixels_ref,
        )?;
        Ok((out_w, out_h, coverage))
    }

    /// Measures the unadjusted linear light in a 5×5 box on `settle_linear` around normalized `(x, y)`
    /// and calculates the temperature and tint shifts required to equalize R, G, and B.
    ///
    /// Refuses with descriptive error if coordinates are out of bounds (not 0.0..=1.0),
    /// if any channel mean >= 0.98 (clipped highlight), or if luminance < 0.005 (deep shadow).
    pub fn white_balance_at(&self, x: f32, y: f32) -> Result<WhiteBalancePick, Error> {
        if !(0.0..=1.0).contains(&x) || !(0.0..=1.0).contains(&y) || x.is_nan() || y.is_nan() {
            return Err(Error::Refused(format!(
                "Sample point ({x:.3}, {y:.3}) is outside stored frame bounds 0.0..=1.0"
            )));
        }

        let w = self.settle_linear.width;
        let h = self.settle_linear.height;
        if w == 0 || h == 0 {
            return Err(Error::Refused("Image buffer is empty".into()));
        }

        let cx = ((x * (w as f32 - 1.0)).round() as i32).clamp(0, w as i32 - 1);
        let cy = ((y * (h as f32 - 1.0)).round() as i32).clamp(0, h as i32 - 1);

        let mut sum_r = 0.0f32;
        let mut sum_g = 0.0f32;
        let mut sum_b = 0.0f32;
        let mut count = 0usize;

        for dy in -2..=2 {
            let py = cy + dy;
            if py < 0 || py >= h as i32 {
                continue;
            }
            for dx in -2..=2 {
                let px = cx + dx;
                if px < 0 || px >= w as i32 {
                    continue;
                }
                let idx = (py as usize * w as usize + px as usize) * 3;
                sum_r += self.settle_linear.data[idx];
                sum_g += self.settle_linear.data[idx + 1];
                sum_b += self.settle_linear.data[idx + 2];
                count += 1;
            }
        }

        if count == 0 {
            return Err(Error::Refused("No valid pixels in sample box".into()));
        }

        let r = sum_r / count as f32;
        let g = sum_g / count as f32;
        let b = sum_b / count as f32;

        if r >= 0.98 || g >= 0.98 || b >= 0.98 {
            return Err(Error::Refused(
                "Cannot balance on clipped highlight (channel mean >= 0.98)".into(),
            ));
        }

        let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        if luma < 0.005 {
            return Err(Error::Refused(
                "Cannot balance on deep shadow / noise floor (luminance < 0.005)".into(),
            ));
        }

        if r <= 1e-6 || g <= 1e-6 || b <= 1e-6 {
            return Err(Error::Refused(
                "Cannot balance on near-zero channel value".into(),
            ));
        }

        let t = (b / r).log2();
        let g_tint = ((g * g) / (r * b)).log2();

        let raw_temp = t * 100.0;
        let raw_tint = g_tint * 100.0;

        let clamped_temp = raw_temp.clamp(-100.0, 100.0);
        let clamped_tint = raw_tint.clamp(-100.0, 100.0);
        let clamped = clamped_temp != raw_temp || clamped_tint != raw_tint;

        Ok(WhiteBalancePick {
            temperature: clamped_temp,
            tint: clamped_tint,
            clamped,
        })
    }

    /// Measures the unadjusted linear light in a 5×5 box on `settle_linear` around normalized `(x, y)`
    /// and returns the OkLCh hue, chroma, and lightness (RP-10).
    ///
    /// Refuses with descriptive error if coordinates are out of bounds (not 0.0..=1.0),
    /// or if chroma < 0.01 (there is no colour there to select).
    pub fn colour_at(&self, x: f32, y: f32) -> Result<super::point_colour::PickedColour, Error> {
        if !(0.0..=1.0).contains(&x) || !(0.0..=1.0).contains(&y) || x.is_nan() || y.is_nan() {
            return Err(Error::Refused(format!(
                "Sample point ({x:.3}, {y:.3}) is outside stored frame bounds 0.0..=1.0"
            )));
        }

        let w = self.settle_linear.width;
        let h = self.settle_linear.height;
        if w == 0 || h == 0 {
            return Err(Error::Refused("Image buffer is empty".into()));
        }

        let cx = ((x * (w as f32 - 1.0)).round() as i32).clamp(0, w as i32 - 1);
        let cy = ((y * (h as f32 - 1.0)).round() as i32).clamp(0, h as i32 - 1);

        let mut sum_r = 0.0f32;
        let mut sum_g = 0.0f32;
        let mut sum_b = 0.0f32;
        let mut count = 0usize;

        for dy in -2..=2 {
            let py = cy + dy;
            if py < 0 || py >= h as i32 {
                continue;
            }
            for dx in -2..=2 {
                let px = cx + dx;
                if px < 0 || px >= w as i32 {
                    continue;
                }
                let idx = (py as usize * w as usize + px as usize) * 3;
                sum_r += self.settle_linear.data[idx];
                sum_g += self.settle_linear.data[idx + 1];
                sum_b += self.settle_linear.data[idx + 2];
                count += 1;
            }
        }

        if count == 0 {
            return Err(Error::Refused("No valid pixels in sample box".into()));
        }

        let r = sum_r / count as f32;
        let g = sum_g / count as f32;
        let b = sum_b / count as f32;

        let (l, a, b_val) = crate::media::edit::color::linear_srgb_to_oklab(r, g, b);
        let chroma = (a * a + b_val * b_val).sqrt();
        if chroma < 0.01 {
            return Err(Error::Refused(
                "Chroma below 0.01: there is no colour there to select".into(),
            ));
        }

        let hue = b_val.atan2(a).to_degrees().rem_euclid(360.0);

        Ok(super::point_colour::PickedColour {
            hue,
            chroma,
            lightness: l.clamp(0.0, 1.0),
        })
    }

    /// Evaluates optimal tone sliders (exposure, whites, blacks) via bisection on the drag proxy
    /// under an identity recipe.
    ///
    /// White balance is intentionally not touched: white balance is an artistic / perceptual
    /// decision; auto-WB from a single scene frame frequently ruins intentional warm sunset
    /// or cool dusk moods; auto-tone focuses strictly on exposure and dynamic range.
    pub fn auto_tone(&self) -> Result<AutoTone, Error> {
        let mut recipe = AdjustmentRecipe::default();

        // Step 1: Exposure in [-2.0, +2.0]
        let frame_zero = render_rgba_frame(&self.drag_linear, &recipe, None)?;
        let med_zero = compute_display_luma_percentile(&frame_zero.bytes, 0.5);
        let mut chosen_exposure = if (95.0..=135.0).contains(&med_zero) {
            0.0
        } else {
            let target = med_zero.clamp(95.0, 135.0);
            let mut low = -2.0f32;
            let mut high = 2.0f32;
            for _ in 0..12 {
                let mid = (low + high) * 0.5;
                recipe.exposure = mid;
                let frame = render_rgba_frame(&self.drag_linear, &recipe, None)?;
                let med = compute_display_luma_percentile(&frame.bytes, 0.5);
                if med < target {
                    low = mid;
                } else {
                    high = mid;
                }
            }
            ((low + high) * 0.5 * 100.0).round() / 100.0
        };

        // If exposure is positive, bisect in [0, exposure] for the 99.5th percentile
        // of display luma at 250, and take the smaller exposure so dark frames with
        // small bright areas do not blow their highlights.
        if chosen_exposure > 0.0 {
            recipe.exposure = chosen_exposure;
            let frame = render_rgba_frame(&self.drag_linear, &recipe, None)?;
            let p995 = compute_display_luma_percentile(&frame.bytes, 0.995);
            if p995 > 250.0 {
                let mut low = 0.0f32;
                let mut high = chosen_exposure;
                for _ in 0..12 {
                    let mid = (low + high) * 0.5;
                    recipe.exposure = mid;
                    let frame = render_rgba_frame(&self.drag_linear, &recipe, None)?;
                    let p = compute_display_luma_percentile(&frame.bytes, 0.995);
                    if p < 250.0 {
                        low = mid;
                    } else {
                        high = mid;
                    }
                }
                let hl_limited_exp = ((low + high) * 0.5 * 100.0).round() / 100.0;
                chosen_exposure = chosen_exposure.min(hl_limited_exp);
            }
        }
        recipe.exposure = chosen_exposure;

        // Step 2: Whites in [-50.0, +50.0]
        let frame_exp = render_rgba_frame(&self.drag_linear, &recipe, None)?;
        let p995_zero = compute_display_luma_percentile(&frame_exp.bytes, 0.995);
        let chosen_whites = if p995_zero < 235.0 {
            let target = 235.0;
            let mut low = -50.0f32;
            let mut high = 50.0f32;
            for _ in 0..12 {
                let mid = (low + high) * 0.5;
                recipe.whites = mid;
                let frame = render_rgba_frame(&self.drag_linear, &recipe, None)?;
                let p = compute_display_luma_percentile(&frame.bytes, 0.995);
                if p < target {
                    low = mid;
                } else {
                    high = mid;
                }
            }
            ((low + high) * 0.5 * 10.0).round() / 10.0
        } else if p995_zero >= 254.0 {
            let target = 250.0;
            let mut low = -50.0f32;
            let mut high = 50.0f32;
            for _ in 0..12 {
                let mid = (low + high) * 0.5;
                recipe.whites = mid;
                let frame = render_rgba_frame(&self.drag_linear, &recipe, None)?;
                let p = compute_display_luma_percentile(&frame.bytes, 0.995);
                if p < target {
                    low = mid;
                } else {
                    high = mid;
                }
            }
            ((low + high) * 0.5 * 10.0).round() / 10.0
        } else {
            0.0
        };
        recipe.whites = chosen_whites;

        // Step 3: Blacks in [-50.0, +50.0]
        let frame_whites = render_rgba_frame(&self.drag_linear, &recipe, None)?;
        let p005_zero = compute_display_luma_percentile(&frame_whites.bytes, 0.005);
        let chosen_blacks = if p005_zero > 12.0 {
            let target = 12.0;
            let mut low = -50.0f32;
            let mut high = 50.0f32;
            for _ in 0..12 {
                let mid = (low + high) * 0.5;
                recipe.blacks = mid;
                let frame = render_rgba_frame(&self.drag_linear, &recipe, None)?;
                let p = compute_display_luma_percentile(&frame.bytes, 0.005);
                if p > target {
                    high = mid;
                } else {
                    low = mid;
                }
            }
            ((low + high) * 0.5 * 10.0).round() / 10.0
        } else {
            0.0
        };

        Ok(AutoTone {
            exposure: chosen_exposure,
            whites: chosen_whites,
            blacks: chosen_blacks,
        })
    }
}

fn compute_display_luma_percentile(bytes: &[u8], percentile: f64) -> f32 {
    let mut hist = [0u32; 256];
    let n_pixels = bytes.len() / 4;
    for px in bytes.as_chunks::<4>().0 {
        let y = (0.2126 * px[0] as f32 + 0.7152 * px[1] as f32 + 0.0722 * px[2] as f32)
            .round()
            .clamp(0.0, 255.0) as usize;
        hist[y] += 1;
    }
    let target_count = ((n_pixels as f64 * percentile).ceil() as u32).max(1);
    let mut acc = 0;
    for (val, &cnt) in hist.iter().enumerate() {
        acc += cnt;
        if acc >= target_count {
            return val as f32;
        }
    }
    255.0
}

/// Calculates the dimensions (width, height) scaled down so the longest edge does not exceed `max_edge`.
///
/// If `w.max(h) <= max_edge` or `max_edge == 0`, returns `(w, h)` unmodified.
pub fn fit_full_edge(w: u32, h: u32, max_edge: u32) -> (u32, u32) {
    let long = w.max(h);
    if long <= max_edge || max_edge == 0 {
        return (w, h);
    }
    let ratio = max_edge as f64 / long as f64;
    let new_w = ((w as f64 * ratio).floor() as u32).max(1);
    let new_h = ((h as f64 * ratio).floor() as u32).max(1);
    (new_w, new_h)
}

/// Downscales an ImageBuffer so its long edge does not exceed `max_edge`.
/// If the image is already within bounds, returns a clone without scaling.
pub fn downscale_image_buffer(buf: &ImageBuffer, max_edge: u32) -> Result<ImageBuffer, Error> {
    let w = buf.width();
    let h = buf.height();
    let (new_w, new_h) = fit_full_edge(w, h, max_edge);
    if new_w == w && new_h == h {
        return Ok(buf.clone());
    }

    match buf {
        ImageBuffer::Rgb8 { data, .. } => {
            let src = fr::images::ImageRef::new(w, h, data, fr::PixelType::U8x3)
                .map_err(|e| Error::Internal(format!("resize source: {e:?}")))?;
            let mut dst = fr::images::Image::new(new_w, new_h, fr::PixelType::U8x3);
            fr::Resizer::new()
                .resize(&src, &mut dst, None)
                .map_err(|e| Error::Internal(format!("resize: {e:?}")))?;
            Ok(ImageBuffer::Rgb8 {
                width: new_w,
                height: new_h,
                data: dst.into_vec(),
            })
        }
        ImageBuffer::Rgb16 { data, .. } => {
            let byte_slice =
                unsafe { std::slice::from_raw_parts(data.as_ptr() as *const u8, data.len() * 2) };
            let src = fr::images::ImageRef::new(w, h, byte_slice, fr::PixelType::U16x3)
                .map_err(|e| Error::Internal(format!("resize source: {e:?}")))?;
            let mut dst = fr::images::Image::new(new_w, new_h, fr::PixelType::U16x3);
            fr::Resizer::new()
                .resize(&src, &mut dst, None)
                .map_err(|e| Error::Internal(format!("resize: {e:?}")))?;
            let dst_bytes = dst.into_vec();
            let mut out16 = Vec::with_capacity(dst_bytes.len() / 2);
            for chunk in dst_bytes.as_chunks::<2>().0.iter() {
                out16.push(u16::from_ne_bytes([chunk[0], chunk[1]]));
            }
            Ok(ImageBuffer::Rgb16 {
                width: new_w,
                height: new_h,
                data: out16,
            })
        }
    }
}

/// Evaluates adjustments, display transfer, and 3D LUT sampling on a linear buffer,
/// writing directly into an RGBA8 output frame in a single multithreaded pass with zero
/// intermediate buffer copies.
pub fn render_rgba_frame(
    proxy: &LinearBuffer,
    recipe: &AdjustmentRecipe,
    lut: Option<&Lut>,
) -> Result<RgbaFrame, Error> {
    // With no geometry the proxy is the stored frame, so it is also what masks are placed on.
    let masks = CompiledMasks::compile(&recipe.masks, None, proxy.width, proxy.height, 1)?;
    render_rgba_frame_masked(proxy, recipe, lut, masks.as_ref())
}

/// The per-pixel constants of pass 1: the global recipe's, or a masked pixel's (ED-19).
#[derive(Clone, Copy)]
struct PixelParams {
    gain_r: f32,
    gain_g: f32,
    gain_b: f32,
    h_val: f32,
    s_val: f32,
    w_val: f32,
    b_val: f32,
    combined_exp: f32,
    combined_scale: f32,
    has_tone_curve: bool,
    sat_term: f32,
    sat_vib: f32,
}

impl PixelParams {
    /// A masked pixel's constants: the global ones, recomputed only where the local
    /// adjustments differ, with `exp2` rather than `powf`. The settle frame has a few
    /// milliseconds per mask (the Round 3 budget), and five `powf` per covered pixel
    /// would spend it several times over.
    #[inline(always)]
    fn with_local(
        &self,
        recipe: &AdjustmentRecipe,
        local: &LocalAdjustments,
        has_brightness: bool,
        bright_gamma: f32,
    ) -> Self {
        let mut p = *self;
        if local.exposure != 0.0 || local.temperature != 0.0 || local.tint != 0.0 {
            let t = (recipe.temperature + local.temperature) / 100.0;
            let g = (recipe.tint + local.tint) / 100.0;
            let e = recipe.exposure + local.exposure;
            p.gain_r = (0.5 * t + e).exp2();
            p.gain_g = (-0.5 * g + e).exp2();
            p.gain_b = (-0.5 * t + e).exp2();
        }
        p.h_val += local.highlights / 100.0;
        p.s_val += local.shadows / 100.0;
        p.w_val += local.whites / 100.0;
        p.b_val += local.blacks / 100.0;
        if local.contrast != 0.0 {
            let gamma_minus_one = 0.5 * (recipe.contrast + local.contrast) / 100.0;
            // (1 / 0.18)^γ−1, as the global path computes it, through exp2.
            p.combined_scale = (-gamma_minus_one * 0.18f32.log2()).exp2();
            p.combined_exp = if has_brightness {
                (bright_gamma - 1.0) + bright_gamma * gamma_minus_one
            } else {
                gamma_minus_one
            };
            p.has_tone_curve = true;
        }
        if local.saturation != 0.0 || local.vibrance != 0.0 {
            p.sat_term = 1.0 + (recipe.saturation + local.saturation) / 100.0;
            p.sat_vib = p.sat_term * (recipe.vibrance + local.vibrance) / 100.0;
        }
        p
    }
}

/// `render_rgba_frame` with masks compiled against the geometry that produced `proxy`.
pub fn render_rgba_frame_masked(
    proxy: &LinearBuffer,
    recipe: &AdjustmentRecipe,
    lut: Option<&Lut>,
    masks: Option<&CompiledMasks>,
) -> Result<RgbaFrame, Error> {
    render_rgba_frame_masked_with_base(proxy, recipe, lut, masks, None)
}

/// As `render_rgba_frame_masked`, reading incoming linear RGB from `base` (when provided)
/// for range mask evaluation, while adjusting `proxy` (RP-11b).
#[allow(clippy::uninit_vec)]
pub fn render_rgba_frame_masked_with_base(
    proxy: &LinearBuffer,
    recipe: &AdjustmentRecipe,
    lut: Option<&Lut>,
    masks: Option<&CompiledMasks>,
    base: Option<&LinearBuffer>,
) -> Result<RgbaFrame, Error> {
    let w = proxy.width;
    let h = proxy.height;
    let pixel_count = (w as usize)
        .checked_mul(h as usize)
        .ok_or_else(|| Error::Internal("image dimensions overflow".into()))?;
    let out_len = pixel_count
        .checked_mul(4)
        .ok_or_else(|| Error::Internal("image byte size overflow".into()))?;

    // Precompute adjustment gains outside the pixel loop
    let t = recipe.temperature / 100.0;
    let g = recipe.tint / 100.0;
    let wb_r = 2.0f32.powf(0.5 * t);
    let wb_b = 2.0f32.powf(-0.5 * t);
    let wb_g = 2.0f32.powf(-0.5 * g);
    let exp_gain = 2.0f32.powf(recipe.exposure);

    let gain_r = wb_r * exp_gain;
    let gain_g = wb_g * exp_gain;
    let gain_b = wb_b * exp_gain;

    let h_val = recipe.highlights / 100.0;
    let s_val = recipe.shadows / 100.0;
    let w_val = recipe.whites / 100.0;
    let b_val = recipe.blacks / 100.0;
    let has_tone = h_val != 0.0 || s_val != 0.0 || w_val != 0.0 || b_val != 0.0;

    let bright = recipe.brightness / 100.0;
    let has_brightness = bright != 0.0;
    let bright_gamma = (-0.5 * bright).exp2();

    let c = recipe.contrast / 100.0;
    let has_contrast = c != 0.0;
    let gamma_minus_one = 0.5 * c;

    let (has_tone_curve, combined_exp, combined_scale) = match (has_brightness, has_contrast) {
        (true, true) => {
            let b_exp = bright_gamma - 1.0;
            let c_scale = (1.0f32 / 0.18f32).powf(gamma_minus_one);
            let comb_exp = b_exp + bright_gamma * gamma_minus_one;
            (true, comb_exp, c_scale)
        }
        (true, false) => {
            let b_exp = bright_gamma - 1.0;
            (true, b_exp, 1.0f32)
        }
        (false, true) => {
            let c_scale = (1.0f32 / 0.18f32).powf(gamma_minus_one);
            (true, gamma_minus_one, c_scale)
        }
        (false, false) => (false, 0.0f32, 1.0f32),
    };

    let sat = recipe.saturation / 100.0;
    let vib = recipe.vibrance / 100.0;
    let has_sat_vib = sat != 0.0 || vib != 0.0;
    let sat_term = 1.0 + sat;
    let sat_vib = sat_term * vib;

    let global = PixelParams {
        gain_r,
        gain_g,
        gain_b,
        h_val,
        s_val,
        w_val,
        b_val,
        combined_exp,
        combined_scale,
        has_tone_curve,
        sat_term,
        sat_vib,
    };
    // A stage at rest globally still runs where a mask is active.
    let has_tone = has_tone || masks.is_some_and(|m| m.touches_tone);
    let has_tone_curve = has_tone_curve || masks.is_some_and(|m| m.touches_contrast);
    let has_sat_vib = has_sat_vib || masks.is_some_and(|m| m.touches_saturation);

    let compiled_hsl = super::hsl::CompiledHslTable::from_recipe_with_bw(
        recipe.hsl.as_ref(),
        recipe.hue,
        recipe.black_and_white.as_ref(),
    );
    let compiled_points = if recipe.black_and_white.is_some() {
        super::point_colour::CompiledPointColours::identity()
    } else {
        super::point_colour::CompiledPointColours::compile(&recipe.point_colours)
    };
    let has_points = !compiled_points.is_identity();
    let has_oklab = !compiled_hsl.is_identity || has_points;

    let intensity = if recipe.lut.is_some() {
        recipe.lut_intensity.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let has_lut = lut.is_some() && intensity > 0.0;
    let lut_is_unit = lut.is_some_and(|l| l.is_unit_domain());

    let curves_table = ToneCurvesTable::from_recipe(recipe.curves.as_ref());
    let grading_table = super::grading::CompiledGradingTable::from_recipe(recipe.grading.as_ref());
    let has_grading = !grading_table.is_identity;
    let grading_lut = &*grading_table.table;
    let srgb_table = &**FAST_LINEAR_TO_SRGB;

    let has_vignette = recipe.vignette.as_ref().is_some_and(|v| !v.is_identity());
    let compiled_vignette = recipe.vignette.as_ref().map(|v| v.compile(w, h)).unwrap_or(
        super::vignette::CompiledVignette {
            xc: 0.0,
            yc: 0.0,
            inv_hx: 1.0,
            inv_hy: 1.0,
            aspect: 1.0,
            d_max: 1.414,
            circ_scale: 1.0,
            d0: 0.0,
            inv_span: 1.0,
            a: 0.0,
            r_param: 0.0,
            is_identity: true,
        },
    );

    let has_grain = recipe.grain.as_ref().is_some_and(|g| !g.is_identity());
    let compiled_grain = recipe
        .grain
        .as_ref()
        .map(|g| g.compile(&recipe.source_sha256))
        .unwrap_or_else(super::grain::CompiledGrain::identity);
    let w_f = w as f32;
    let inv_w = 1.0 / w_f.max(1.0);

    let looks_ref = recipe.looks.as_ref();
    let has_bloom = looks_ref.is_some_and(|l| l.has_glow() || l.has_halation());
    let has_tone_mapper = looks_ref.is_some_and(|l| l.has_tone_mapper());

    let bloom_buf = if has_bloom {
        super::looks::build_combined_bloom_with_gains(
            proxy,
            looks_ref.unwrap(),
            gain_r,
            gain_g,
            gain_b,
            h_val,
            w_val,
        )
    } else {
        None
    };

    let (bloom_w_f, bloom_h_f, inv_h) = if let Some(ref b) = bloom_buf {
        (b.width as f32, b.height as f32, 1.0 / (h as f32).max(1.0))
    } else {
        (1.0, 1.0, 1.0)
    };
    let inv_w_bloom = inv_w * bloom_w_f;

    let mut out_bytes = Vec::with_capacity(out_len);
    unsafe {
        out_bytes.set_len(out_len);
    }

    let row_out_len = (w * 4) as usize;
    let row_in_len = (w * 3) as usize;

    out_bytes
        .par_chunks_exact_mut(row_out_len)
        .zip(proxy.data.par_chunks_exact(row_in_len))
        .enumerate()
        .for_each(|(y, (out_row, in_row))| {
            let y_f = y as f32;
            let v_vignette = (y_f + 0.5 - compiled_vignette.yc) * compiled_vignette.inv_hy;
            let v2_vignette = v_vignette * v_vignette;
            let bloom_sampler = bloom_buf.as_ref().map(|b| {
                let v_bloom = (y_f + 0.5) * inv_h * bloom_h_f - 0.5;
                b.row_sampler(v_bloom)
            });
            let grain_rp = if has_grain {
                let py1 = (y_f + 0.5) * inv_w * compiled_grain.k_base;
                compiled_grain.row_params(py1)
            } else {
                super::grain::RowGrainParams::default()
            };
            let mask_row = masks.map(|m| (m, m.row(y as u32, w, h)));
            let base_row = base.map(|b| {
                let start = y * (w as usize) * 3;
                &b.data[start..start + (w as usize) * 3]
            });
            const CHUNK_SIZE: usize = 64;
            let mut lin_buf = [[0.0f32; 3]; CHUNK_SIZE];

            for (chunk_idx, (out_c, in_c)) in out_row
                .chunks_mut(CHUNK_SIZE * 4)
                .zip(in_row.chunks(CHUNK_SIZE * 3))
                .enumerate()
            {
                let base_x = chunk_idx * CHUNK_SIZE;
                let count = in_c.len() / 3;

                // Pass 1: Linear adjustments + Oklab stage + Vignette (low register pressure, no display/LUT/packing)
                for (i, (slot, in_px)) in lin_buf[..count]
                    .iter_mut()
                    .zip(in_c.as_chunks::<3>().0.iter())
                    .enumerate()
                {
                    let local_params;
                    let p = match mask_row {
                        Some((m, cursor)) => {
                            let (mx, my) = cursor.at(base_x + i);
                            let (r_base, g_base, b_base) = match base_row {
                                Some(br) => {
                                    let idx = (base_x + i) * 3;
                                    (br[idx], br[idx + 1], br[idx + 2])
                                }
                                None => (in_px[0], in_px[1], in_px[2]),
                            };
                            let local = m.local_at(mx, my, r_base, g_base, b_base);
                            if local.is_zero() {
                                &global
                            } else {
                                local_params =
                                    global.with_local(recipe, &local, has_brightness, bright_gamma);
                                &local_params
                            }
                        }
                        None => &global,
                    };

                    let mut r = in_px[0] * p.gain_r;
                    let mut g = in_px[1] * p.gain_g;
                    let mut b = in_px[2] * p.gain_b;

                    let mut y = 0.2126 * r + 0.7152 * g + 0.0722 * b;

                    // 3. Highlights, Shadows, Whites, Blacks (crossover at 0.18, 0.18 strictly stationary)
                    if has_tone {
                        const INV_H: f32 = 1.0 / (0.65 - 0.18);
                        const INV_W: f32 = 1.0 / (1.0 - 0.18);
                        const INV_S: f32 = 1.0 / (0.18 - 0.05);
                        const INV_B: f32 = 1.0 / (0.18 - 0.02);

                        let delta_ev = if y >= 0.18 {
                            let w_h = if y >= 0.65 {
                                1.0
                            } else {
                                let t = (y - 0.18) * INV_H;
                                t * t * (3.0 - 2.0 * t)
                            };
                            let w_w = if y >= 1.0 {
                                1.0
                            } else {
                                let t = (y - 0.18) * INV_W;
                                t * t * (3.0 - 2.0 * t)
                            };
                            w_h * p.h_val + w_w * p.w_val
                        } else {
                            let w_s = if y <= 0.05 {
                                1.0
                            } else {
                                let t = (0.18 - y) * INV_S;
                                t * t * (3.0 - 2.0 * t)
                            };
                            let w_b = if y <= 0.02 {
                                1.0
                            } else {
                                let t = (0.18 - y) * INV_B;
                                t * t * (3.0 - 2.0 * t)
                            };
                            w_s * p.s_val + w_b * p.b_val
                        };
                        if delta_ev != 0.0 {
                            let factor = delta_ev.exp2();
                            r *= factor;
                            g *= factor;
                            b *= factor;
                            y *= factor;
                        }
                    }

                    // 4. Brightness & Contrast (combined power curve pivoting on mid-grey)
                    if has_tone_curve
                        && p.has_tone_curve
                        && y > 1e-7
                        && (!has_brightness || (y - 1.0).abs() > 1e-7)
                    {
                        let factor = (p.combined_exp * y.log2()).exp2() * p.combined_scale;
                        r *= factor;
                        g *= factor;
                        b *= factor;
                        y *= factor;
                    }

                    // 5. Saturation & Vibrance (preserves neutral grey)
                    // Per pixel as well as per frame: where no mask covers, a stage switched on
                    // for the masks skips the pixel, as the unmasked path would. Applying it
                    // with k = 1 differs only by float rounding, which 8-bit output hides, so
                    // no test can see this; it keeps the claim exact rather than nearly so.
                    if has_sat_vib && (p.sat_term != 1.0 || p.sat_vib != 0.0) {
                        let max = r.max(g).max(b);
                        let min = r.min(g).min(b);
                        let current_sat = if max > 1e-7 {
                            ((max - min) / max).min(1.0)
                        } else {
                            0.0
                        };
                        let k = (p.sat_term + p.sat_vib * (1.0 - current_sat)).max(0.0);
                        r = (y + k * (r - y)).max(0.0);
                        g = (y + k * (g - y)).max(0.0);
                        b = (y + k * (b - y)).max(0.0);
                    }

                    // Oklab stage: Point colour, Global Hue rotation, and 8-band HSL in ONE pass
                    if has_oklab {
                        if !has_points {
                            let (hr, hg, hb) = compiled_hsl.apply_linear_srgb(r, g, b);
                            r = hr;
                            g = hg;
                            b = hb;
                        } else {
                            let (l, a, b_val) =
                                crate::media::edit::color::linear_srgb_to_oklab(r, g, b);
                            let (l_pt, a_pt, b_pt) = compiled_points.apply(l, a, b_val);
                            let (final_l, final_a, final_b) = if !compiled_hsl.is_identity {
                                let c_sq = a_pt * a_pt + b_pt * b_pt;
                                compiled_hsl.apply_with_c_sq(l_pt, a_pt, b_pt, c_sq)
                            } else {
                                (l_pt, a_pt, b_pt)
                            };
                            let (hr, hg, hb) = crate::media::edit::color::oklab_to_linear_srgb(
                                final_l, final_a, final_b,
                            );
                            r = hr.max(0.0);
                            g = hg.max(0.0);
                            b = hb.max(0.0);
                        }
                    }

                    // Scale-independent vignette in linear light
                    if has_vignette {
                        let x_f = (base_x + i) as f32;
                        let v_gain = compiled_vignette.gain_with_v(x_f, v_vignette, v2_vignette);
                        r *= v_gain;
                        g *= v_gain;
                        b *= v_gain;
                    }

                    // Photographic looks: Glow and Halation added in scene-linear light
                    if let Some(ref sampler) = bloom_sampler {
                        let x_f = (base_x + i) as f32;
                        let u_bloom = (x_f + 0.5) * inv_w_bloom - 0.5;
                        let b_val = sampler.sample(u_bloom);
                        r += b_val[0];
                        g += b_val[1];
                        b += b_val[2];
                    }

                    // Filmic tone mapper: Narkowicz ACES compresses dynamic range into [0.0, 1.0]
                    if has_tone_mapper {
                        r = super::looks::aces_narkowicz(r);
                        g = super::looks::aces_narkowicz(g);
                        b = super::looks::aces_narkowicz(b);
                    }

                    *slot = [r, g, b];
                }

                // Pass 2: Display transfer + Tone curves + 3D LUT + Film Grain + byte packing (no linear math or Oklab)
                for (i, (out_px, &[r, g, b])) in out_c
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(&lin_buf[..count])
                    .enumerate()
                {
                    // Display transfer (linear light to display-encoded sRGB [0.0, 1.0])
                    let disp_r = fast_linear_to_srgb_table(r, srgb_table);
                    let disp_g = fast_linear_to_srgb_table(g, srgb_table);
                    let disp_b = fast_linear_to_srgb_table(b, srgb_table);

                    // Tone curves in display-encoded space
                    let (curved_r, curved_g, curved_b) = if curves_table.has_any {
                        curves_table.apply(disp_r, disp_g, disp_b)
                    } else {
                        (disp_r, disp_g, disp_b)
                    };

                    // 3-way colour grading in display-encoded space
                    let (graded_r, graded_g, graded_b) = if has_grading {
                        super::grading::CompiledGradingTable::apply_table(
                            grading_lut,
                            curved_r,
                            curved_g,
                            curved_b,
                        )
                    } else {
                        (curved_r, curved_g, curved_b)
                    };

                    // 3D LUT sampling & blending in display space
                    let (final_r, final_g, final_b) = if has_lut {
                        let lut_inst = lut.unwrap();
                        let lut_out = if lut_is_unit {
                            lut_inst.sample_unit([graded_r, graded_g, graded_b])
                        } else {
                            lut_inst.sample([graded_r, graded_g, graded_b])
                        };
                        (
                            graded_r + intensity * (lut_out[0] - graded_r),
                            graded_g + intensity * (lut_out[1] - graded_g),
                            graded_b + intensity * (lut_out[2] - graded_b),
                        )
                    } else {
                        (graded_r, graded_g, graded_b)
                    };

                    // Scale-independent film grain in display domain (luminance only)
                    let (grain_r, grain_g, grain_b) = if has_grain {
                        let x_f = (base_x + i) as f32;
                        let u = (x_f + 0.5) * inv_w;
                        let delta = compiled_grain
                            .sample_delta_with_row(u, &grain_rp, final_r, final_g, final_b);
                        (final_r + delta, final_g + delta, final_b + delta)
                    } else {
                        (final_r, final_g, final_b)
                    };

                    let r_u8 = (grain_r.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                    let g_u8 = (grain_g.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                    let b_u8 = (grain_b.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                    out_px.copy_from_slice(&[r_u8, g_u8, b_u8, 255]);
                }
            }
        });

    let histogram = histogram(&out_bytes, w, h)?;
    Ok(RgbaFrame {
        width: w,
        height: h,
        bytes: out_bytes,
        orientation: 1,
        histogram,
        to_stored: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
    })
}
