//! The managed LUT library in `core` (ED-6).
//!
//! Stores imported 3D Look-Up Tables in a dedicated managed directory under
//! the application data folder (`~/.local/share/masterphototools/luts` or
//! `~/Library/Application Support/masterphototools/luts`).
//!
//! All operations are atomic, non-destructive, and validate LUT formats
//! before disk modification.

use crate::error::Error;
use crate::media::edit::lut::Lut;
use crate::media::edit::pipeline::LutRef;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Metadata describing an imported, parseable 3D LUT in the library.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LutEntry {
    pub name: String,
    pub sha256: String,
    pub format: String,
}

/// An unparseable or corrupted file encountered in the LUT library folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LutError {
    pub name: String,
    pub error: String,
}

/// The contents of the managed LUT library: valid LUTs and any invalid files.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LutLibraryList {
    pub luts: Vec<LutEntry>,
    pub errors: Vec<LutError>,
}

/// Detects format label from filename extension.
pub fn format_from_name(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".cube") {
        "cube".to_string()
    } else if lower.ends_with(".3dl") {
        "3dl".to_string()
    } else if lower.ends_with(".png") {
        "hald_png".to_string()
    } else {
        "unknown".to_string()
    }
}

/// Lists all LUT files in the managed library directory.
///
/// Unparseable files are returned in `errors` rather than silently hidden (G10).
pub fn list_luts(library_dir: &Path) -> Result<LutLibraryList, Error> {
    if !library_dir.exists() {
        return Ok(LutLibraryList::default());
    }

    let mut luts = Vec::new();
    let mut errors = Vec::new();

    let entries = std::fs::read_dir(library_dir)?;
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let file_name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        // Skip hidden files (.DS_Store, etc.)
        if file_name.starts_with('.') {
            continue;
        }

        match Lut::from_file(&path) {
            Ok(lut) => {
                let format = format_from_name(&file_name);
                luts.push(LutEntry {
                    name: file_name,
                    sha256: lut.sha256,
                    format,
                });
            }
            Err(e) => {
                errors.push(LutError {
                    name: file_name,
                    error: e.to_string(),
                });
            }
        }
    }

    luts.sort_by(|a, b| a.name.cmp(&b.name));
    errors.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(LutLibraryList { luts, errors })
}

/// Imports a LUT into the managed library.
///
/// 1. Parses the source LUT first, refusing unparseable files before creating anything.
/// 2. If a file with the same name already exists:
///    - If it has the exact same SHA-256 hash, import succeeds idempotently without re-writing.
///    - If it has a different SHA-256 hash, import is refused to prevent overwriting a different LUT.
/// 3. Otherwise, copies the file into `library_dir` with `create_new(true)`.
pub fn import_lut(library_dir: &Path, source_path: &Path) -> Result<LutEntry, Error> {
    // Parse first: fail immediately if the LUT is invalid or corrupted.
    let lut = Lut::from_file(source_path)?;

    std::fs::create_dir_all(library_dir)?;

    let file_name = source_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| Error::Refused(format!("Invalid file path: {}", source_path.display())))?;

    let dest_path = library_dir.join(file_name);

    if dest_path.exists() {
        // Read existing file and check hash
        let existing = match Lut::from_file(&dest_path) {
            Ok(l) => l.sha256,
            Err(_) => {
                // If existing file is corrupted or unparseable, refuse overwrite
                return Err(Error::Refused(format!(
                    "Cannot import '{file_name}': a different file with that name already exists in the library"
                )));
            }
        };

        if existing != lut.sha256 {
            return Err(Error::Refused(format!(
                "Cannot import '{file_name}': a different LUT with that name already exists in the library"
            )));
        }

        return Ok(LutEntry {
            name: file_name.to_string(),
            sha256: lut.sha256,
            format: format_from_name(file_name),
        });
    }

    // Atomic create_new: never overwrite existing files.
    let bytes = std::fs::read(source_path)?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&dest_path)?;
    file.write_all(&bytes)?;

    Ok(LutEntry {
        name: file_name.to_string(),
        sha256: lut.sha256,
        format: format_from_name(file_name),
    })
}

/// Finds a LUT in the library directory matching `sha256`.
pub fn find_lut_by_sha256(
    library_dir: &Path,
    sha256: &str,
) -> Result<Option<(PathBuf, Lut)>, Error> {
    if !library_dir.exists() {
        return Ok(None);
    }
    let entries = std::fs::read_dir(library_dir)?;
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if let Ok(lut) = Lut::from_file(&path) {
            if lut.sha256 == sha256 {
                return Ok(Some((path, lut)));
            }
        }
    }
    Ok(None)
}

/// Finds a LUT in the library by sha256 or filename.
pub fn find_lut_by_name_or_sha256(
    library_dir: &Path,
    query: &str,
) -> Result<Option<(PathBuf, Lut)>, Error> {
    if !library_dir.exists() {
        return Ok(None);
    }

    // First try exact sha256 match
    if let Some(found) = find_lut_by_sha256(library_dir, query)? {
        return Ok(Some(found));
    }

    // Next try direct filename join
    let candidate = library_dir.join(query);
    if candidate.is_file() {
        if let Ok(lut) = Lut::from_file(&candidate) {
            return Ok(Some((candidate, lut)));
        }
    }

    // Next scan files for filename match
    let entries = std::fs::read_dir(library_dir)?;
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if path.is_file() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name == query {
                    if let Ok(lut) = Lut::from_file(&path) {
                        return Ok(Some((path, lut)));
                    }
                }
            }
        }
    }

    Ok(None)
}

/// Resolves a recipe's `LutRef` to the concrete LUT file and parsed `Lut` from the library.
///
/// Looks up strictly by SHA-256. If the LUT is not found (e.g. deleted from library),
/// returns a refusal error naming the missing LUT.
pub fn resolve_recipe_lut(library_dir: &Path, lut_ref: &LutRef) -> Result<(PathBuf, Lut), Error> {
    match find_lut_by_sha256(library_dir, &lut_ref.sha256)? {
        Some(found) => Ok(found),
        None => Err(Error::Refused(format!(
            "LUT '{}' is no longer in the library",
            lut_ref.name
        ))),
    }
}
