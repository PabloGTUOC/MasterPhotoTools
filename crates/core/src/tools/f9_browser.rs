use crate::config::Config;
use crate::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserEntry {
    pub name: String,
    pub absolute_path: PathBuf,
    pub is_dir: bool,
    pub size: Option<u64>,
}

pub fn list_directory(config: &Config, request_path: &Path) -> Result<Vec<BrowserEntry>, Error> {
    let resolved = config.resolve(request_path)?;

    let mut entries = Vec::new();

    // Add ".." if not at one of the roots
    let is_root = config.roots.iter().any(|r| r == &resolved);
    if !is_root {
        if let Some(parent) = resolved.parent() {
            if config.resolve(parent).is_ok() {
                entries.push(BrowserEntry {
                    name: "..".to_string(),
                    absolute_path: parent.to_path_buf(),
                    is_dir: true,
                    size: None,
                });
            }
        }
    }

    let read_dir = match fs::read_dir(&resolved) {
        Ok(rd) => rd,
        Err(_) => return Ok(entries),
    };

    let mut children = Vec::new();

    for entry in read_dir {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();
        // Skip unreadable metadata
        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };

        let is_dir = metadata.is_dir();
        let size = if is_dir { None } else { Some(metadata.len()) };
        let name = entry.file_name().to_string_lossy().to_string();

        children.push(BrowserEntry {
            name,
            absolute_path: path,
            is_dir,
            size,
        });
    }

    // Sort: directories first, then alphabetical case-insensitive
    children.sort_by(|a, b| {
        if a.is_dir != b.is_dir {
            if a.is_dir {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            }
        } else {
            a.name.to_lowercase().cmp(&b.name.to_lowercase())
        }
    });

    entries.extend(children);

    Ok(entries)
}
