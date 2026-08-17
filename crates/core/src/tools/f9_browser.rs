//! F9 — library browser.
//!
//! Browsing is confined to the configured roots (G6, §9.2 rule 2): every path
//! is canonicalised and rejected unless it resolves inside one.

use crate::config::Config;
use crate::error::Error;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

        // Follow symlinks deliberately: `DirEntry::metadata` uses `lstat`, which
        // succeeds for a link whose target is gone. An entry that cannot be read
        // is skipped rather than listed as something the caller cannot open.
        let metadata = match fs::metadata(&path) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Thresholds;
    use std::os::unix::fs::symlink;
    use tempfile::tempdir;

    fn config_rooted_at(root: &Path) -> Config {
        Config {
            roots: vec![root.canonicalize().unwrap()],
            staging_dir: PathBuf::new(),
            thresholds: Thresholds::default(),
            database: PathBuf::new(),
        }
    }

    #[test]
    fn directories_sort_first_then_case_insensitive_alphabetical() {
        let temp = tempdir().unwrap();
        let root = temp.path().join("root");
        fs::create_dir(&root).unwrap();

        for dir in ["Zebra", "apple"] {
            fs::create_dir(root.join(dir)).unwrap();
        }
        for file in ["Banana.jpg", "apricot.jpg"] {
            fs::write(root.join(file), "x").unwrap();
        }

        let config = config_rooted_at(&root);
        let names: Vec<String> = list_directory(&config, &root)
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();

        assert_eq!(names, ["apple", "Zebra", "apricot.jpg", "Banana.jpg"]);
    }

    #[test]
    fn a_parent_entry_appears_below_a_root_but_not_at_it() {
        let temp = tempdir().unwrap();
        let root = temp.path().join("root");
        let child = root.join("child");
        fs::create_dir_all(&child).unwrap();

        let config = config_rooted_at(&root);

        let at_root = list_directory(&config, &root).unwrap();
        assert!(
            !at_root.iter().any(|e| e.name == ".."),
            "a root must not offer a way out of itself"
        );

        let below = list_directory(&config, &child).unwrap();
        assert_eq!(below.first().map(|e| e.name.as_str()), Some(".."));
    }

    #[test]
    fn files_report_a_size_and_directories_do_not() {
        let temp = tempdir().unwrap();
        let root = temp.path().join("root");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("a.jpg"), b"12345").unwrap();
        fs::create_dir(root.join("sub")).unwrap();

        let config = config_rooted_at(&root);
        let entries = list_directory(&config, &root).unwrap();

        let dir = entries.iter().find(|e| e.name == "sub").unwrap();
        assert!(dir.is_dir);
        assert_eq!(dir.size, None);

        let file = entries.iter().find(|e| e.name == "a.jpg").unwrap();
        assert!(!file.is_dir);
        assert_eq!(file.size, Some(5));
    }

    /// G6, end to end through the tool rather than at `Config::resolve` alone.
    #[test]
    fn browsing_outside_a_root_is_rejected() {
        let temp = tempdir().unwrap();
        let root = temp.path().join("root");
        let outside = temp.path().join("outside");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret.jpg"), "x").unwrap();

        let config = config_rooted_at(&root);

        // Directly outside.
        assert!(list_directory(&config, &outside).is_err());

        // Via `..` traversal.
        let traversal = root.join("..").join("outside");
        assert!(list_directory(&config, &traversal).is_err());

        // Via a symlink that escapes.
        let escape = root.join("escape");
        symlink(&outside, &escape).unwrap();
        assert!(
            list_directory(&config, &escape).is_err(),
            "a symlink out of a root must not be followed"
        );
    }

    #[test]
    fn an_unreadable_entry_is_skipped_rather_than_failing_the_listing() {
        let temp = tempdir().unwrap();
        let root = temp.path().join("root");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("good.jpg"), "x").unwrap();

        // A broken symlink cannot be stat'ed; the listing must survive it.
        symlink(root.join("nowhere"), root.join("broken")).unwrap();

        let config = config_rooted_at(&root);
        let entries = list_directory(&config, &root).unwrap();

        assert!(entries.iter().any(|e| e.name == "good.jpg"));
        assert!(!entries.iter().any(|e| e.name == "broken"));
    }
}
