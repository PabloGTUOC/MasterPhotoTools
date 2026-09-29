//! Image editing tool layer (ED-3).
//!
//! Provides sidecar persistence (.photoedit), volume root detection (G5 card safety),
//! and full-resolution export with collision protection and metadata preservation.
//!
//! Drives `media::edit` and never touches pixels itself.

use crate::error::Error;
use crate::media::edit::{
    apply_recipe, decode_image, validate_lut, AdjustmentRecipe, ImageBuffer, Lut,
};
use crate::tools::{carry_metadata, Derived, Skip};
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub const SIDECAR_EXTENSION: &str = "photoedit";
pub const CURRENT_RECIPE_VERSION: u32 = 1;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Compute the companion sidecar path for an image path.
///
/// Follows `<file name>.photoedit` (e.g. `IMG_0001.JPG.photoedit` or `IMG_0001.CR2.photoedit`),
/// ensuring RAW and JPEG pairs side-by-side do not share a sidecar.
pub fn sidecar_path(image_path: &Path) -> PathBuf {
    let file_name = image_path.file_name().unwrap_or_default().to_string_lossy();
    image_path.with_file_name(format!("{file_name}.{SIDECAR_EXTENSION}"))
}

/// Returns true if the path represents a `.photoedit` sidecar.
pub fn is_sidecar(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.ends_with(".photoedit"))
}

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
    match crate::ingest::card::Card::at(&root) {
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

    let mut to_save = recipe.clone();
    if to_save.source_sha256.is_empty() {
        let bytes = std::fs::read(image_path)?;
        to_save.source_sha256 = crate::ingest::scanner::hex(&Sha256::digest(&bytes));
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
    recipe.exposure == 0.0
        && recipe.temperature == 0.0
        && recipe.tint == 0.0
        && recipe.highlights == 0.0
        && recipe.shadows == 0.0
        && recipe.contrast == 0.0
        && recipe.saturation == 0.0
        && recipe.vibrance == 0.0
        && (recipe.lut.is_none() || recipe.lut_intensity == 0.0)
        && lut.is_none()
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

/// Atomically creates an exclusive export target file, testing candidates `_edit`, `_edit_1`, `_edit_2`...
fn exclusive_create_export_target(
    out_dir: &Path,
    stem: &str,
    ext: &str,
) -> Result<(PathBuf, File), Error> {
    let mut k = 0usize;
    loop {
        let filename = match (k, ext.is_empty()) {
            (0, false) => format!("{stem}_edit.{ext}"),
            (0, true) => format!("{stem}_edit"),
            (n, false) => format!("{stem}_edit_{n}.{ext}"),
            (n, true) => format!("{stem}_edit_{n}"),
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

/// Export an edited image to `out_dir` with non-overwriting monotonic naming (`_edit`, `_edit_1`...).
///
/// Refuses export to an SD card volume (G5). The caller is responsible for validating that `out_dir`
/// has already been resolved against allowed roots and does not land inside the Publishing folder
/// (e.g. using `resolve_output`, as applied at the command layer in ED-6).
///
/// Identity recipes on non-RAW images perform a direct stream byte copy to avoid generational compression loss.
pub fn export_edited_image(
    source: &Path,
    recipe: &AdjustmentRecipe,
    lut: Option<&Lut>,
    out_dir: &Path,
) -> Result<ExportResult, Error> {
    export_edited_image_with_resolver(source, recipe, lut, out_dir, find_volume_root)
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
    // 1. Refuse destination on an SD card volume (G5)
    if is_card_volume_with_resolver(out_dir, &root_resolver) {
        return Err(Error::Refused(
            "Cannot export to a card volume: card media is read-only (G5)".into(),
        ));
    }

    // 2. Validate LUT presence: a recipe that requests a LUT must have it provided and matched
    validate_lut(recipe, lut)?;

    let stem = source.file_stem().unwrap_or_default().to_string_lossy();
    if stem.is_empty() {
        return Err(Error::Refused("Source file has no stem name".into()));
    }

    let is_raw = crate::media::raw::is_raw(source);
    let identity = is_identity(recipe, lut);

    // Identity recipe on already-encoded non-RAW image: byte copy
    if !is_raw && identity {
        let mut src_file = File::open(source)?;
        std::fs::create_dir_all(out_dir)?;

        let ext = source
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        let (target_path, mut target_file) = exclusive_create_export_target(out_dir, &stem, &ext)?;
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

    // Non-identity or RAW: decode, apply, encode to memory first so no empty file is left behind
    let decoded = decode_image(source)?;
    let processed = apply_recipe(&decoded, recipe, lut)?;

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
                exclusive_create_export_target(out_dir, &stem, "tiff")?;
            if let Err(e) = target_file.write_all(&tiff_bytes) {
                let _ = std::fs::remove_file(&target_path);
                return Err(Error::from(e));
            }
            if let Err(e) = target_file.flush() {
                let _ = std::fs::remove_file(&target_path);
                return Err(Error::from(e));
            }
            drop(target_file);

            let derived = [Derived {
                source: source.to_path_buf(),
                output: target_path.clone(),
                width,
                height,
            }];
            let skipped = carry_metadata(&derived, false);
            let metadata_skipped = skipped.into_iter().next();

            Ok(ExportResult {
                path: target_path,
                metadata_skipped,
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
            let jpeg_bytes = crate::media::jpeg::encode(
                &dyn_img,
                &crate::media::jpeg::JpegOptions::deliverable(95),
            )?;

            std::fs::create_dir_all(out_dir)?;
            let (target_path, mut target_file) =
                exclusive_create_export_target(out_dir, &stem, "jpg")?;
            if let Err(e) = target_file.write_all(&jpeg_bytes) {
                let _ = std::fs::remove_file(&target_path);
                return Err(Error::from(e));
            }
            if let Err(e) = target_file.flush() {
                let _ = std::fs::remove_file(&target_path);
                return Err(Error::from(e));
            }
            drop(target_file);

            let derived = [Derived {
                source: source.to_path_buf(),
                output: target_path.clone(),
                width,
                height,
            }];
            let skipped = carry_metadata(&derived, false);
            let metadata_skipped = skipped.into_iter().next();

            Ok(ExportResult {
                path: target_path,
                metadata_skipped,
            })
        }
    }
}
