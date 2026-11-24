//! Folder operations for Edit (UI-8).
//!
//! Lists editable images in a directory (JPEGs, TIFFs, RAWs) and marks those with
//! existing `.photoedit` sidecars. Generates upright thumbnails for all supported types.

use std::path::Path;

use phototools_core::error::Error;
use serde::{Deserialize, Serialize};

/// An editable entry in a folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditableEntry {
    pub name: String,
    pub edited: bool,
}

fn is_editable_extension(ext: &str) -> bool {
    let lower = ext.to_lowercase();
    matches!(lower.as_str(), "jpg" | "jpeg" | "tif" | "tiff")
        || phototools_core::media::raw::RAW_EXTENSIONS.contains(&lower.as_str())
}

/// Returns the list of editable file names in `folder`, non-recursive, sorted by byte order.
pub fn editable_names(folder: &Path) -> Result<Vec<EditableEntry>, Error> {
    if !folder.is_dir() {
        return Err(Error::Refused(format!(
            "{} is not a directory",
            folder.display()
        )));
    }
    let entries = std::fs::read_dir(folder).map_err(Error::Io)?;
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(Error::Io)?;
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if is_editable_extension(ext) {
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        let sidecar = phototools_core::sidecar::sidecar_path(&path);
                        let edited = sidecar.is_file();
                        names.push(EditableEntry {
                            name: name.to_string(),
                            edited,
                        });
                    }
                }
            }
        }
    }
    names.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(names)
}

/// Generates an upright thumbnail JPEG fit to `max_edge`.
///
/// RAW files delegate to `develop_folder::thumbnail`.
/// JPEGs and TIFFs decode with orientation applied, downscale to `max_edge`,
/// and encode using deliverable JPEG options.
pub fn edit_thumbnail(path: &Path, max_edge: u32) -> Result<Vec<u8>, Error> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    if phototools_core::media::raw::RAW_EXTENSIONS.contains(&ext.as_str()) {
        crate::tools::develop_folder::thumbnail(path, max_edge)
    } else {
        let upright = phototools_core::media::image_ops::decode_oriented(path)?;
        let scaled = phototools_core::media::image_ops::downscale_to_max_edge(&upright, max_edge)?;
        let opts = phototools_core::media::jpeg::JpegOptions::deliverable(85);
        phototools_core::media::jpeg::encode(&scaled, &opts)
    }
}

#[cfg(test)]
#[path = "../../../core/tests/fixtures/mod.rs"]
mod fixtures;

#[cfg(test)]
mod tests {
    use super::*;
    use fixtures::Fixtures;
    use std::fs;

    #[test]
    fn a_folder_lists_jpegs_tiffs_and_raws_and_nothing_else() {
        let temp = tempfile::tempdir().unwrap();
        let folder = temp.path();

        fs::write(folder.join("a.JPG"), b"").unwrap();
        fs::write(folder.join("b.tif"), b"").unwrap();
        fs::write(folder.join("c.RW2"), b"").unwrap();
        fs::write(folder.join("d.txt"), b"").unwrap();
        fs::write(folder.join("e.png"), b"").unwrap();
        fs::create_dir(folder.join("subfolder")).unwrap();

        let entries = editable_names(folder).unwrap();
        let names: Vec<String> = entries.into_iter().map(|e| e.name).collect();
        assert_eq!(names, vec!["a.JPG", "b.tif", "c.RW2"]);
    }

    #[test]
    fn an_edited_photograph_is_marked() {
        let temp = tempfile::tempdir().unwrap();
        let folder = temp.path();

        let a_path = folder.join("a.JPG");
        fs::write(&a_path, b"").unwrap();
        fs::write(folder.join("b.tif"), b"").unwrap();

        let sidecar = phototools_core::sidecar::sidecar_path(&a_path);
        fs::write(sidecar, b"{}").unwrap();

        let entries = editable_names(folder).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0],
            EditableEntry {
                name: "a.JPG".to_string(),
                edited: true,
            }
        );
        assert_eq!(
            entries[1],
            EditableEntry {
                name: "b.tif".to_string(),
                edited: false,
            }
        );
    }

    #[test]
    fn a_jpeg_thumbnail_is_upright_and_fits() {
        let f = Fixtures::new();
        // Orientation 6: 90 degrees CW rotation. When decoded upright, 60x30 becomes 30x60 (portrait).
        let path = f.jpeg_with_orientation("oriented.jpg", 60, 30, 6);
        let max_edge = 40;
        let thumb_bytes = edit_thumbnail(&path, max_edge).unwrap();

        let thumb = image::load_from_memory(&thumb_bytes).unwrap();
        assert!(
            thumb.height() > thumb.width(),
            "expected portrait thumbnail, got {}x{}",
            thumb.width(),
            thumb.height()
        );
        assert_eq!(thumb.height(), max_edge);
    }

    #[test]
    fn a_missing_folder_is_refused() {
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("missing_dir");
        let result = editable_names(&missing);
        assert!(matches!(result, Err(Error::Refused(_))));
    }
}
