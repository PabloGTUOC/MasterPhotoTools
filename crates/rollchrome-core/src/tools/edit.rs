//! Image editing tool layer (ED-3).
//!
//! Provides sidecar persistence (.photoedit), volume root detection (G5 card safety),
//! and full-resolution export with collision protection and metadata preservation.
//!
//! Drives `media::edit` and never touches pixels itself.

use crate::media::edit::detail::{apply_detail, Detail};
use crate::media::edit::icc::srgb_metadata_corrections;
use crate::media::edit::pipeline::{linear_to_u16, linear_to_u8, LinearBuffer};
use crate::media::edit::{
    apply_recipe_with_orientation, decode_image, validate_lut, AdjustmentRecipe, ImageBuffer, Lut,
};
use fast_image_resize as fr;
use image::ImageEncoder;
use phototools_core::error::Error;
use phototools_core::tools::{carry_metadata_correcting, Derived, Skip};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub use phototools_core::sidecar::{is_sidecar, sidecar_path, SIDECAR_EXTENSION};

/// Version 3 added masks (ED-19). A sidecar is written at this version even without
/// masks, so an older build refuses every sidecar a newer one wrote rather than opening
/// one and silently dropping what it does not know.
pub const CURRENT_RECIPE_VERSION: u32 = 3;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Find the volume root (mount point) for a path.
///
/// On Unix, walks up ancestors until the filesystem device id (`dev`) differs
/// from its parent's device id, or reaches filesystem root.
/// On non-Unix, falls back to the path's root component.
pub fn find_volume_root(path: &Path) -> Option<PathBuf> {
    let mut current = if path.exists() {
        path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
    } else {
        let mut cur = path.to_path_buf();
        while !cur.exists() {
            match cur.parent() {
                Some(p) => cur = p.to_path_buf(),
                None => break,
            }
        }
        cur.canonicalize().unwrap_or(cur)
    };

    if current.is_file() {
        if let Some(parent) = current.parent() {
            current = parent.to_path_buf();
        }
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let mut cur_dev = match std::fs::metadata(&current) {
            Ok(m) => m.dev(),
            Err(_) => return None,
        };

        while let Some(parent) = current.parent() {
            match std::fs::metadata(parent) {
                Ok(parent_meta) => {
                    if parent_meta.dev() != cur_dev {
                        return Some(current);
                    }
                    cur_dev = parent_meta.dev();
                    current = parent.to_path_buf();
                }
                Err(_) => break,
            }
        }
        Some(current)
    }

    #[cfg(not(unix))]
    {
        current
            .components()
            .next()
            .map(|c| PathBuf::from(c.as_os_str()))
    }
}

/// Returns true if the path resides on an SD card volume (identified by `DCIM` at the volume root).
pub fn is_card_volume(path: &Path) -> bool {
    is_card_volume_with_resolver(path, find_volume_root)
}

/// Returns true if the path resides on an SD card volume using an injectable root resolver.
pub fn is_card_volume_with_resolver<F>(path: &Path, resolve_root: F) -> bool
where
    F: Fn(&Path) -> Option<PathBuf>,
{
    let Some(root) = resolve_root(path) else {
        return false;
    };
    match phototools_core::ingest::card::Card::at(&root) {
        Ok(card) => card.looks_like_a_card(),
        Err(_) => false,
    }
}

/// Load an `AdjustmentRecipe` from a `.photoedit` JSON sidecar.
///
/// An unknown future version is refused clearly and never guessed.
pub fn load_recipe(sidecar_path: &Path) -> Result<AdjustmentRecipe, Error> {
    let bytes = std::fs::read(sidecar_path)?;
    let val: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|e| Error::Refused(format!("invalid sidecar JSON: {e}")))?;

    let version = val.get("version").and_then(|v| v.as_u64()).unwrap_or(1) as u32;

    if version > CURRENT_RECIPE_VERSION {
        return Err(Error::Refused(format!(
            "unsupported sidecar version {version} (maximum supported is {CURRENT_RECIPE_VERSION})"
        )));
    }

    let recipe: AdjustmentRecipe = serde_json::from_value(val)
        .map_err(|e| Error::Refused(format!("malformed sidecar recipe: {e}")))?;

    Ok(recipe)
}

/// Load the companion recipe for an image, or None if no sidecar exists.
pub fn load_recipe_for_image(image_path: &Path) -> Result<Option<AdjustmentRecipe>, Error> {
    let sidecar = sidecar_path(image_path);
    if !sidecar.exists() {
        return Ok(None);
    }
    load_recipe(&sidecar).map(Some)
}

/// Save an `AdjustmentRecipe` into its companion `<file name>.photoedit` sidecar atomically.
///
/// Refuses to write on an SD card volume (G5).
pub fn save_recipe(image_path: &Path, recipe: &AdjustmentRecipe) -> Result<PathBuf, Error> {
    save_recipe_with_resolver(image_path, recipe, find_volume_root)
}

/// Save an `AdjustmentRecipe` using an injectable volume root resolver.
pub fn save_recipe_with_resolver<F>(
    image_path: &Path,
    recipe: &AdjustmentRecipe,
    root_resolver: F,
) -> Result<PathBuf, Error>
where
    F: Fn(&Path) -> Option<PathBuf>,
{
    if is_card_volume_with_resolver(image_path, &root_resolver) {
        return Err(Error::Refused(
            "Card media is read-only (G5). Copy files to a working folder to save edits.".into(),
        ));
    }

    if !image_path.exists() {
        return Err(Error::Refused(format!(
            "Cannot save sidecar: image does not exist: {}",
            image_path.display()
        )));
    }

    if recipe.point_colours.len() > 8 {
        return Err(Error::Refused(
            "Cannot save more than 8 point colours".into(),
        ));
    }

    let mut to_save = recipe.clone();
    to_save.version = CURRENT_RECIPE_VERSION;
    if to_save.detail.as_ref().is_some_and(|d| d.is_identity()) {
        to_save.detail = None;
    }
    // Clear snapshots and versions inside each snapshot's own recipe so that they never nest (RP-5, RP-6).
    for snap in &mut to_save.snapshots {
        if snap.recipe.detail.as_ref().is_some_and(|d| d.is_identity()) {
            snap.recipe.detail = None;
        }
        snap.recipe.snapshots.clear();
        snap.recipe.versions.clear();
        snap.recipe.version_name = None;
    }
    // Clear versions and snapshots inside each version's own recipe so that they never nest (RP-6).
    for ver in &mut to_save.versions {
        if ver.recipe.detail.as_ref().is_some_and(|d| d.is_identity()) {
            ver.recipe.detail = None;
        }
        ver.recipe.versions.clear();
        ver.recipe.snapshots.clear();
        ver.recipe.version_name = None;
    }
    if to_save.source_sha256.is_empty() {
        let bytes = std::fs::read(image_path)?;
        to_save.source_sha256 = phototools_core::ingest::scanner::hex(&Sha256::digest(&bytes));
    }

    let target_path = sidecar_path(image_path);
    let parent = target_path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;

    let json_bytes = serde_json::to_vec_pretty(&to_save)
        .map_err(|e| Error::Internal(format!("failed to serialize recipe: {e}")))?;

    // Atomic write via temporary file in the same directory, then rename
    let pid = std::process::id();
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temp_name = format!(".photoedit_tmp_{pid}_{counter}");
    let temp_path = parent.join(temp_name);

    let mut temp_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp_path)?;

    if let Err(e) = temp_file.write_all(&json_bytes) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(Error::from(e));
    }
    if let Err(e) = temp_file.flush() {
        let _ = std::fs::remove_file(&temp_path);
        return Err(Error::from(e));
    }
    drop(temp_file);

    if let Err(e) = std::fs::rename(&temp_path, &target_path) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(Error::from(e));
    }

    Ok(target_path)
}

/// Returns true if the recipe represents exact identity (all adjustments at rest, no effective LUT).
pub fn is_identity(recipe: &AdjustmentRecipe, lut: Option<&Lut>) -> bool {
    recipe.is_identity() && lut.is_none()
}

/// The result of an export operation.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExportResult {
    pub path: PathBuf,
    pub metadata_skipped: Option<Skip>,
}

impl std::ops::Deref for ExportResult {
    type Target = Path;
    fn deref(&self) -> &Self::Target {
        &self.path
    }
}

impl AsRef<Path> for ExportResult {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

/// Turn a version name into a safe file path slug keeping letters and digits, turning anything else into '-' (RP-6).
pub fn slug_version_name(name: &str) -> String {
    let mut slug = String::with_capacity(name.len());
    for c in name.chars() {
        if c.is_alphanumeric() {
            slug.push(c);
        } else {
            slug.push('-');
        }
    }
    slug
}

/// Options controlling output format, resizing, sharpening, naming, and metadata on export (RP-9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ExportOptions {
    #[serde(default)]
    pub format: ExportFormat,
    #[serde(default)]
    pub resize: ExportResize,
    #[serde(default)]
    pub output_sharpening: OutputSharpening,
    #[serde(default)]
    pub name_template: Option<String>,
    #[serde(default)]
    pub metadata: MetadataPolicy,
}

/// The encoding format and format-specific parameters for exported images.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ExportFormat {
    #[default]
    AsSource,
    Jpeg {
        quality: u8,
    },
    Tiff {
        bits: u8,
    },
    Png {
        bits: u8,
    },
    Webp,
    Avif {
        quality: u8,
    },
}

/// The spatial resizing constraint applied to exported images (never enlarges).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ExportResize {
    #[default]
    None,
    LongEdge(u32),
    Megapixels(u32),
}

/// Output sharpening tailored for display medium.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum OutputSharpening {
    #[default]
    Off,
    Screen,
    Print,
}

/// Metadata preservation policy on export derivatives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MetadataPolicy {
    #[default]
    All,
    NoLocation,
    None,
}

/// Atomically creates an exclusive export target file, testing candidates `{stem}_{suffix}.{ext}`, `{stem}_{suffix}_1.{ext}`, etc.
pub fn exclusive_create_target(
    out_dir: &Path,
    stem: &str,
    suffix: &str,
    ext: &str,
) -> Result<(PathBuf, File), Error> {
    let norm_suffix = if suffix.is_empty() {
        String::new()
    } else if suffix.starts_with('_') {
        suffix.to_string()
    } else {
        format!("_{suffix}")
    };
    let mut k = 0usize;
    loop {
        let filename = match (k, ext.is_empty()) {
            (0, false) => format!("{stem}{norm_suffix}.{ext}"),
            (0, true) => format!("{stem}{norm_suffix}"),
            (n, false) => format!("{stem}{norm_suffix}_{n}.{ext}"),
            (n, true) => format!("{stem}{norm_suffix}_{n}"),
        };
        let candidate_path = out_dir.join(&filename);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate_path)
        {
            Ok(file) => return Ok((candidate_path, file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                k += 1;
            }
            Err(e) => return Err(Error::from(e)),
        }
    }
}

/// Resolves a filename template substituting tokens `{name}`, `{version}`, `{date}`, `{edit}` (RP-9).
/// Sanitizes anything other than alphanumeric, `-`, `_`, `.`, and spaces to `-` (G6).
/// Refuses a template that resolves to empty or only dots.
pub fn resolve_name_template(
    template: &str,
    source: &Path,
    recipe: &AdjustmentRecipe,
) -> Result<String, Error> {
    let stem = source.file_stem().unwrap_or_default().to_string_lossy();
    let version = recipe.version_name.as_deref().unwrap_or("");
    let date = phototools_core::media::meta::read_meta(source)
        .ok()
        .and_then(|m| m.capture)
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_default();
    let edit_suffix = match &recipe.version_name {
        Some(name) => format!("_edit_{}", slug_version_name(name)),
        None => "_edit".to_string(),
    };

    let substituted = template
        .replace("{name}", &stem)
        .replace("{version}", version)
        .replace("{date}", &date)
        .replace("{edit}", &edit_suffix);

    let mut sanitized = String::with_capacity(substituted.len());
    for c in substituted.chars() {
        if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' || c == ' ' {
            sanitized.push(c);
        } else {
            sanitized.push('-');
        }
    }

    let trimmed = sanitized.trim();
    if trimmed.is_empty() || trimmed.chars().all(|c| c == '.') {
        return Err(Error::Refused(
            "Export name template cannot result in an empty filename or only dots".into(),
        ));
    }

    Ok(sanitized)
}

fn resize_image_buffer(buf: ImageBuffer, new_w: u32, new_h: u32) -> Result<ImageBuffer, Error> {
    let (w, h) = (buf.width(), buf.height());
    if w == new_w && h == new_h {
        return Ok(buf);
    }
    match buf {
        ImageBuffer::Rgb8 { data, .. } => {
            let src = fr::images::ImageRef::new(w, h, &data, fr::PixelType::U8x3)
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

fn apply_output_sharpening(buf: ImageBuffer, detail: &Detail) -> ImageBuffer {
    let lin = LinearBuffer::from_image_buffer(&buf);
    let sharpened = apply_detail(&lin, detail, 1.0);
    match buf {
        ImageBuffer::Rgb8 { width, height, .. } => {
            let mut out = vec![0u8; sharpened.data.len()];
            out.par_chunks_exact_mut(3)
                .zip(sharpened.data.par_chunks_exact(3))
                .for_each(|(out_px, in_px)| {
                    out_px[0] = linear_to_u8(in_px[0]);
                    out_px[1] = linear_to_u8(in_px[1]);
                    out_px[2] = linear_to_u8(in_px[2]);
                });
            ImageBuffer::Rgb8 {
                width,
                height,
                data: out,
            }
        }
        ImageBuffer::Rgb16 { width, height, .. } => {
            let mut out = vec![0u16; sharpened.data.len()];
            out.par_chunks_exact_mut(3)
                .zip(sharpened.data.par_chunks_exact(3))
                .for_each(|(out_px, in_px)| {
                    out_px[0] = linear_to_u16(in_px[0]);
                    out_px[1] = linear_to_u16(in_px[1]);
                    out_px[2] = linear_to_u16(in_px[2]);
                });
            ImageBuffer::Rgb16 {
                width,
                height,
                data: out,
            }
        }
    }
}

fn encode_for_export(
    buffer: &ImageBuffer,
    format: ExportFormat,
) -> Result<(&'static str, Vec<u8>), Error> {
    match format {
        ExportFormat::AsSource => match buffer {
            ImageBuffer::Rgb16 {
                width,
                height,
                data,
            } => {
                let img16 = image::ImageBuffer::<image::Rgb<u16>, Vec<u16>>::from_raw(
                    *width,
                    *height,
                    data.clone(),
                )
                .ok_or_else(|| Error::Internal("failed to construct Rgb16 buffer".into()))?;
                let dyn_img = image::DynamicImage::ImageRgb16(img16);
                let mut tiff_bytes = Vec::new();
                let mut cursor = std::io::Cursor::new(&mut tiff_bytes);
                dyn_img
                    .write_to(&mut cursor, image::ImageFormat::Tiff)
                    .map_err(|e| Error::Internal(format!("failed to encode TIFF: {e}")))?;
                Ok(("tiff", tiff_bytes))
            }
            ImageBuffer::Rgb8 {
                width,
                height,
                data,
            } => {
                let dyn_img = image::DynamicImage::ImageRgb8(
                    image::ImageBuffer::from_raw(*width, *height, data.clone())
                        .ok_or_else(|| Error::Internal("failed to construct Rgb8 buffer".into()))?,
                );
                let jpeg_bytes = phototools_core::media::jpeg::encode(
                    &dyn_img,
                    &phototools_core::media::jpeg::JpegOptions::deliverable(95),
                )?;
                Ok(("jpg", jpeg_bytes))
            }
        },
        ExportFormat::Jpeg { quality } => {
            if !(1..=100).contains(&quality) {
                return Err(Error::Refused(
                    "JPEG quality must be between 1 and 100".into(),
                ));
            }
            let dyn_img = match buffer {
                ImageBuffer::Rgb8 {
                    width,
                    height,
                    data,
                } => image::DynamicImage::ImageRgb8(
                    image::ImageBuffer::from_raw(*width, *height, data.clone())
                        .ok_or_else(|| Error::Internal("failed to construct Rgb8 buffer".into()))?,
                ),
                ImageBuffer::Rgb16 {
                    width,
                    height,
                    data,
                } => {
                    let rgb8: Vec<u8> = data.par_iter().map(|&v| (v >> 8) as u8).collect();
                    image::DynamicImage::ImageRgb8(
                        image::ImageBuffer::from_raw(*width, *height, rgb8).ok_or_else(|| {
                            Error::Internal("failed to construct Rgb8 buffer".into())
                        })?,
                    )
                }
            };
            let jpeg_bytes = phototools_core::media::jpeg::encode(
                &dyn_img,
                &phototools_core::media::jpeg::JpegOptions::deliverable(quality),
            )?;
            Ok(("jpg", jpeg_bytes))
        }
        ExportFormat::Tiff { bits } => {
            if bits != 8 && bits != 16 {
                return Err(Error::Refused("TIFF bit depth must be 8 or 16".into()));
            }
            let (width, height) = (buffer.width(), buffer.height());
            let mut tiff_bytes = Vec::new();
            let mut cursor = std::io::Cursor::new(&mut tiff_bytes);
            if bits == 16 {
                let data16 = match buffer {
                    ImageBuffer::Rgb16 { data, .. } => data.clone(),
                    ImageBuffer::Rgb8 { data, .. } => data
                        .par_iter()
                        .map(|&b| ((b as u16) << 8) | (b as u16))
                        .collect(),
                };
                let img16 = image::ImageBuffer::<image::Rgb<u16>, Vec<u16>>::from_raw(
                    width, height, data16,
                )
                .ok_or_else(|| Error::Internal("failed to construct Rgb16 buffer".into()))?;
                let dyn_img = image::DynamicImage::ImageRgb16(img16);
                dyn_img
                    .write_to(&mut cursor, image::ImageFormat::Tiff)
                    .map_err(|e| Error::Internal(format!("failed to encode TIFF: {e}")))?;
            } else {
                let data8 = match buffer {
                    ImageBuffer::Rgb8 { data, .. } => data.clone(),
                    ImageBuffer::Rgb16 { data, .. } => {
                        data.par_iter().map(|&v| (v >> 8) as u8).collect()
                    }
                };
                let img8 =
                    image::ImageBuffer::<image::Rgb<u8>, Vec<u8>>::from_raw(width, height, data8)
                        .ok_or_else(|| Error::Internal("failed to construct Rgb8 buffer".into()))?;
                let dyn_img = image::DynamicImage::ImageRgb8(img8);
                dyn_img
                    .write_to(&mut cursor, image::ImageFormat::Tiff)
                    .map_err(|e| Error::Internal(format!("failed to encode TIFF: {e}")))?;
            }
            Ok(("tiff", tiff_bytes))
        }
        ExportFormat::Png { bits } => {
            if bits != 8 && bits != 16 {
                return Err(Error::Refused("PNG bit depth must be 8 or 16".into()));
            }
            let (width, height) = (buffer.width(), buffer.height());
            let mut png_bytes = Vec::new();
            let mut cursor = std::io::Cursor::new(&mut png_bytes);
            if bits == 16 {
                let data16 = match buffer {
                    ImageBuffer::Rgb16 { data, .. } => data.clone(),
                    ImageBuffer::Rgb8 { data, .. } => data
                        .par_iter()
                        .map(|&b| ((b as u16) << 8) | (b as u16))
                        .collect(),
                };
                let img16 = image::ImageBuffer::<image::Rgb<u16>, Vec<u16>>::from_raw(
                    width, height, data16,
                )
                .ok_or_else(|| Error::Internal("failed to construct Rgb16 buffer".into()))?;
                let dyn_img = image::DynamicImage::ImageRgb16(img16);
                dyn_img
                    .write_to(&mut cursor, image::ImageFormat::Png)
                    .map_err(|e| Error::Internal(format!("failed to encode PNG: {e}")))?;
            } else {
                let data8 = match buffer {
                    ImageBuffer::Rgb8 { data, .. } => data.clone(),
                    ImageBuffer::Rgb16 { data, .. } => {
                        data.par_iter().map(|&v| (v >> 8) as u8).collect()
                    }
                };
                let img8 =
                    image::ImageBuffer::<image::Rgb<u8>, Vec<u8>>::from_raw(width, height, data8)
                        .ok_or_else(|| Error::Internal("failed to construct Rgb8 buffer".into()))?;
                let dyn_img = image::DynamicImage::ImageRgb8(img8);
                dyn_img
                    .write_to(&mut cursor, image::ImageFormat::Png)
                    .map_err(|e| Error::Internal(format!("failed to encode PNG: {e}")))?;
            }
            Ok(("png", png_bytes))
        }
        ExportFormat::Webp => {
            let (width, height) = (buffer.width(), buffer.height());
            let data8 = match buffer {
                ImageBuffer::Rgb8 { data, .. } => data.clone(),
                ImageBuffer::Rgb16 { data, .. } => {
                    data.par_iter().map(|&v| (v >> 8) as u8).collect()
                }
            };
            let mut bytes = Vec::new();
            let cursor = std::io::Cursor::new(&mut bytes);
            let encoder = image::codecs::webp::WebPEncoder::new_lossless(cursor);
            encoder
                .write_image(&data8, width, height, image::ExtendedColorType::Rgb8)
                .map_err(|e| Error::Internal(format!("failed to encode WebP: {e}")))?;
            Ok(("webp", bytes))
        }
        ExportFormat::Avif { quality } => {
            if !(1..=100).contains(&quality) {
                return Err(Error::Refused(
                    "AVIF quality must be between 1 and 100".into(),
                ));
            }
            let (width, height) = (buffer.width(), buffer.height());
            let data8 = match buffer {
                ImageBuffer::Rgb8 { data, .. } => data.clone(),
                ImageBuffer::Rgb16 { data, .. } => {
                    data.par_iter().map(|&v| (v >> 8) as u8).collect()
                }
            };
            let mut bytes = Vec::new();
            let cursor = std::io::Cursor::new(&mut bytes);
            let encoder =
                image::codecs::avif::AvifEncoder::new_with_speed_quality(cursor, 6, quality);
            encoder
                .write_image(&data8, width, height, image::ExtendedColorType::Rgb8)
                .map_err(|e| Error::Internal(format!("failed to encode AVIF: {e}")))?;
            Ok(("avif", bytes))
        }
    }
}

/// Renders an edited image and writes it to `out_dir` with exclusive naming using `suffix`.
///
/// Decodes the source, applies the adjustment recipe (including LUT validation), encodes
/// to memory (TIFF for 16-bit, JPEG for 8-bit), and writes to an exclusive output file.
/// Returns the `Derived` descriptor without writing metadata.
pub fn render_and_write(
    source: &Path,
    recipe: &AdjustmentRecipe,
    lut: Option<&Lut>,
    out_dir: &Path,
    suffix: &str,
) -> Result<Derived, Error> {
    validate_lut(recipe, lut)?;

    let stem = source.file_stem().unwrap_or_default().to_string_lossy();
    if stem.is_empty() {
        return Err(Error::Refused("Source file has no stem name".into()));
    }

    let orientation = phototools_core::media::meta::read_meta(source)
        .map(|m| m.orientation as u32)
        .unwrap_or(1);
    let decoded = decode_image(source)?;

    let mut resolved_recipe = recipe.clone();
    if resolved_recipe.source_sha256.is_empty() {
        if let Ok(bytes) = std::fs::read(source) {
            resolved_recipe.source_sha256 =
                phototools_core::ingest::scanner::hex(&Sha256::digest(&bytes));
        }
    }

    let processed = apply_recipe_with_orientation(&decoded, &resolved_recipe, lut, orientation)?;

    match processed {
        ImageBuffer::Rgb16 {
            width,
            height,
            data,
        } => {
            let img16 =
                image::ImageBuffer::<image::Rgb<u16>, Vec<u16>>::from_raw(width, height, data)
                    .ok_or_else(|| Error::Internal("failed to construct Rgb16 buffer".into()))?;
            let dyn_img = image::DynamicImage::ImageRgb16(img16);
            let mut tiff_bytes = Vec::new();
            let mut cursor = std::io::Cursor::new(&mut tiff_bytes);
            dyn_img
                .write_to(&mut cursor, image::ImageFormat::Tiff)
                .map_err(|e| Error::Internal(format!("failed to encode TIFF: {e}")))?;

            std::fs::create_dir_all(out_dir)?;
            let (target_path, mut target_file) =
                exclusive_create_target(out_dir, &stem, suffix, "tiff")?;
            if let Err(e) = target_file.write_all(&tiff_bytes) {
                let _ = std::fs::remove_file(&target_path);
                return Err(Error::from(e));
            }
            if let Err(e) = target_file.flush() {
                let _ = std::fs::remove_file(&target_path);
                return Err(Error::from(e));
            }

            Ok(Derived {
                source: source.to_path_buf(),
                output: target_path,
                width,
                height,
            })
        }
        ImageBuffer::Rgb8 {
            width,
            height,
            data,
        } => {
            let dyn_img = image::DynamicImage::ImageRgb8(
                image::ImageBuffer::from_raw(width, height, data)
                    .ok_or_else(|| Error::Internal("failed to construct Rgb8 buffer".into()))?,
            );
            let jpeg_bytes = phototools_core::media::jpeg::encode(
                &dyn_img,
                &phototools_core::media::jpeg::JpegOptions::deliverable(95),
            )?;

            std::fs::create_dir_all(out_dir)?;
            let (target_path, mut target_file) =
                exclusive_create_target(out_dir, &stem, suffix, "jpg")?;
            if let Err(e) = target_file.write_all(&jpeg_bytes) {
                let _ = std::fs::remove_file(&target_path);
                return Err(Error::from(e));
            }
            if let Err(e) = target_file.flush() {
                let _ = std::fs::remove_file(&target_path);
                return Err(Error::from(e));
            }

            Ok(Derived {
                source: source.to_path_buf(),
                output: target_path,
                width,
                height,
            })
        }
    }
}

/// Export an edited image to `out_dir` with non-overwriting monotonic naming (`_edit`, `_edit_1`...).
///
/// Refuses export to an SD card volume (G5). The caller is responsible for validating that `out_dir`
/// has already been resolved against allowed roots and does not land inside the Publishing folder
/// (e.g. using `resolve_output`, as applied at the command layer in ED-6).
///
/// Identity recipes perform a direct stream byte copy to avoid generational compression loss.
pub fn export_edited_image(
    source: &Path,
    recipe: &AdjustmentRecipe,
    lut: Option<&Lut>,
    out_dir: &Path,
) -> Result<ExportResult, Error> {
    export_edited_image_with_options(source, recipe, lut, out_dir, &ExportOptions::default())
}

/// Export an edited image with an injectable volume root resolver.
pub fn export_edited_image_with_resolver<F>(
    source: &Path,
    recipe: &AdjustmentRecipe,
    lut: Option<&Lut>,
    out_dir: &Path,
    root_resolver: F,
) -> Result<ExportResult, Error>
where
    F: Fn(&Path) -> Option<PathBuf>,
{
    export_edited_image_with_options_with_resolver(
        source,
        recipe,
        lut,
        out_dir,
        &ExportOptions::default(),
        root_resolver,
    )
}

/// Export an edited image with export options (RP-9).
pub fn export_edited_image_with_options(
    source: &Path,
    recipe: &AdjustmentRecipe,
    lut: Option<&Lut>,
    out_dir: &Path,
    options: &ExportOptions,
) -> Result<ExportResult, Error> {
    export_edited_image_with_options_with_resolver(
        source,
        recipe,
        lut,
        out_dir,
        options,
        find_volume_root,
    )
}

/// Export an edited image with export options and an injectable volume root resolver (RP-9).
pub fn export_edited_image_with_options_with_resolver<F>(
    source: &Path,
    recipe: &AdjustmentRecipe,
    lut: Option<&Lut>,
    out_dir: &Path,
    options: &ExportOptions,
    root_resolver: F,
) -> Result<ExportResult, Error>
where
    F: Fn(&Path) -> Option<PathBuf>,
{
    // 1. Refuse destination on an SD card volume (G5)
    if is_card_volume_with_resolver(out_dir, &root_resolver) {
        return Err(Error::Refused(
            "Cannot export to a card volume: card media is read-only (G5)".into(),
        ));
    }

    // Refuse RAW source: RAW files are developed in Develop tab only
    if phototools_core::media::raw::is_raw(source) {
        return Err(Error::Refused(
            "RAW files are developed in the Develop tab. Open the JPEG or TIFF it writes.".into(),
        ));
    }

    // 2. Validate LUT presence: a recipe that requests a LUT must have it provided and matched
    validate_lut(recipe, lut)?;

    // Validate parameters up front
    match options.format {
        ExportFormat::Jpeg { quality } if !(1..=100).contains(&quality) => {
            return Err(Error::Refused(
                "JPEG quality must be between 1 and 100".into(),
            ));
        }
        ExportFormat::Avif { quality } if !(1..=100).contains(&quality) => {
            return Err(Error::Refused(
                "AVIF quality must be between 1 and 100".into(),
            ));
        }
        ExportFormat::Tiff { bits } if bits != 8 && bits != 16 => {
            return Err(Error::Refused("TIFF bit depth must be 8 or 16".into()));
        }
        ExportFormat::Png { bits } if bits != 8 && bits != 16 => {
            return Err(Error::Refused("PNG bit depth must be 8 or 16".into()));
        }
        _ => {}
    }

    match options.resize {
        ExportResize::LongEdge(0) => {
            return Err(Error::Refused(
                "Target size must be greater than zero".into(),
            ));
        }
        ExportResize::Megapixels(0) => {
            return Err(Error::Refused(
                "Target size must be greater than zero".into(),
            ));
        }
        _ => {}
    }

    let stem = source.file_stem().unwrap_or_default().to_string_lossy();
    if stem.is_empty() {
        return Err(Error::Refused("Source file has no stem name".into()));
    }

    let default_suffix = match &recipe.version_name {
        Some(name) => format!("_edit_{}", slug_version_name(name)),
        None => "_edit".to_string(),
    };

    let (target_stem, target_suffix) = if let Some(template) = &options.name_template {
        let resolved = resolve_name_template(template, source, recipe)?;
        (resolved, String::new())
    } else {
        (stem.to_string(), default_suffix.clone())
    };

    let identity = is_identity(recipe, lut);
    let is_default_options = options == &ExportOptions::default();

    // Identity recipe on already-encoded image with default options: byte copy without re-encoding
    if identity && is_default_options {
        let mut src_file = File::open(source)?;
        std::fs::create_dir_all(out_dir)?;

        let ext = source
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        let (target_path, mut target_file) =
            exclusive_create_target(out_dir, &target_stem, &target_suffix, &ext)?;
        if let Err(e) = std::io::copy(&mut src_file, &mut target_file) {
            let _ = std::fs::remove_file(&target_path);
            return Err(Error::from(e));
        }
        if let Err(e) = target_file.flush() {
            let _ = std::fs::remove_file(&target_path);
            return Err(Error::from(e));
        }
        return Ok(ExportResult {
            path: target_path,
            metadata_skipped: None,
        });
    }

    // Default options fast path for non-identity: uses render_and_write directly for exact parity
    if is_default_options {
        let derived = render_and_write(source, recipe, lut, out_dir, &default_suffix)?;
        let geometry_active = recipe.geometry.is_some();
        let corrections = srgb_metadata_corrections();
        let skipped = carry_metadata_correcting(
            std::slice::from_ref(&derived),
            geometry_active,
            None,
            &corrections,
        );
        let metadata_skipped = skipped.into_iter().next();
        return Ok(ExportResult {
            path: derived.output,
            metadata_skipped,
        });
    }

    // Non-default options: render pipeline
    let orientation = phototools_core::media::meta::read_meta(source)
        .map(|m| m.orientation as u32)
        .unwrap_or(1);
    let decoded = decode_image(source)?;

    let mut resolved_recipe = recipe.clone();
    if resolved_recipe.source_sha256.is_empty() {
        if let Ok(bytes) = std::fs::read(source) {
            resolved_recipe.source_sha256 =
                phototools_core::ingest::scanner::hex(&Sha256::digest(&bytes));
        }
    }

    let mut buffer = apply_recipe_with_orientation(&decoded, &resolved_recipe, lut, orientation)?;

    // Resize (never enlarges)
    let (cur_w, cur_h) = (buffer.width(), buffer.height());
    let target_dims = match options.resize {
        ExportResize::None => None,
        ExportResize::LongEdge(max_len) => {
            let long = cur_w.max(cur_h);
            if long <= max_len {
                None
            } else if cur_w >= cur_h {
                let tw = max_len;
                let th = ((cur_h as f64 * max_len as f64) / cur_w as f64)
                    .round()
                    .max(1.0) as u32;
                Some((tw, th))
            } else {
                let th = max_len;
                let tw = ((cur_w as f64 * max_len as f64) / cur_h as f64)
                    .round()
                    .max(1.0) as u32;
                Some((tw, th))
            }
        }
        ExportResize::Megapixels(mp) => {
            phototools_core::media::image_ops::dimensions_for_megapixels(cur_w, cur_h, mp)
        }
    };

    if let Some((tw, th)) = target_dims {
        if tw != cur_w || th != cur_h {
            buffer = resize_image_buffer(buffer, tw, th)?;
        }
    }

    // Output sharpening
    match options.output_sharpening {
        OutputSharpening::Off => {}
        OutputSharpening::Screen => {
            let detail = Detail {
                sharpen_amount: 40.0,
                sharpen_radius: 0.7,
                sharpen_masking: 0.0,
                ..Detail::default()
            };
            buffer = apply_output_sharpening(buffer, &detail);
        }
        OutputSharpening::Print => {
            let detail = Detail {
                sharpen_amount: 70.0,
                sharpen_radius: 1.0,
                sharpen_masking: 0.0,
                ..Detail::default()
            };
            buffer = apply_output_sharpening(buffer, &detail);
        }
    }

    // Format encoding
    let (ext, encoded_bytes) = encode_for_export(&buffer, options.format)?;

    // Write to target
    std::fs::create_dir_all(out_dir)?;
    let (target_path, mut target_file) =
        exclusive_create_target(out_dir, &target_stem, &target_suffix, ext)?;
    if let Err(e) = target_file.write_all(&encoded_bytes) {
        let _ = std::fs::remove_file(&target_path);
        return Err(Error::from(e));
    }
    if let Err(e) = target_file.flush() {
        let _ = std::fs::remove_file(&target_path);
        return Err(Error::from(e));
    }

    let derived = Derived {
        source: source.to_path_buf(),
        output: target_path,
        width: buffer.width(),
        height: buffer.height(),
    };

    // Metadata policy
    let metadata_skipped = match options.metadata {
        MetadataPolicy::None => None,
        MetadataPolicy::All => {
            let geometry_active = recipe.geometry.is_some();
            let corrections = srgb_metadata_corrections();
            let skipped = carry_metadata_correcting(
                std::slice::from_ref(&derived),
                geometry_active,
                None,
                &corrections,
            );
            skipped.into_iter().next()
        }
        MetadataPolicy::NoLocation => {
            let geometry_active = recipe.geometry.is_some();
            let mut corrections = srgb_metadata_corrections();
            corrections.push("-gps:all=".to_string());
            let skipped = carry_metadata_correcting(
                std::slice::from_ref(&derived),
                geometry_active,
                None,
                &corrections,
            );
            skipped.into_iter().next()
        }
    };

    Ok(ExportResult {
        path: derived.output,
        metadata_skipped,
    })
}
