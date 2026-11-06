//! Companion `.photoedit` sidecar naming.
//!
//! Lives in `phototools-core` because it is the one contract both applications must agree on —
//! a photograph edited in Rollchrome (`rollchrome-core`) and renamed in PhotoTools (F3) must keep
//! its edits.

use std::path::{Path, PathBuf};

pub const SIDECAR_EXTENSION: &str = "photoedit";

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
