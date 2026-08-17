//! Settings, roots, thresholds

use crate::error::Error;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thresholds {
    pub max_age_days: i64,
    pub max_megapixels: u32,
    pub max_output_bytes: u64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            max_age_days: 90,
            max_megapixels: 10,
            max_output_bytes: 10 * 1024 * 1024, // 10 MB
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub roots: Vec<PathBuf>,
    pub staging_dir: PathBuf,
    pub thresholds: Thresholds,
    pub database: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            roots: vec![],
            staging_dir: dirs::cache_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("masterphototools/staging"),
            thresholds: Thresholds::default(),
            database: dirs::data_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("masterphototools/db.sqlite3"),
        }
    }
}

impl Config {
    pub fn config_path() -> PathBuf {
        let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        path.push("masterphototools");
        path.push("config.json");
        path
    }

    pub fn load() -> Result<Self, Error> {
        let path = Self::config_path();
        if path.exists() {
            let data = std::fs::read_to_string(path).map_err(Error::Io)?;
            serde_json::from_str(&data).map_err(|e| Error::Internal(e.to_string()))
        } else {
            Self::from_env()
        }
    }

    pub fn save(&self) -> Result<(), Error> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(Error::Io)?;
        }
        let data =
            serde_json::to_string_pretty(self).map_err(|e| Error::Internal(e.to_string()))?;
        std::fs::write(path, data).map_err(Error::Io)?;
        Ok(())
    }

    /// Load configuration from environment variables, using defaults where appropriate.
    pub fn from_env() -> Result<Self, Error> {
        let roots_str = std::env::var("ROOTS").unwrap_or_default();
        let roots: Vec<PathBuf> = roots_str
            .split(':')
            .filter(|s| !s.is_empty())
            .map(|s| {
                PathBuf::from(s)
                    .canonicalize()
                    .unwrap_or_else(|_| PathBuf::from(s))
            })
            .collect();

        let staging_dir = PathBuf::from(
            std::env::var("STAGING_DIR").unwrap_or_else(|_| "/tmp/phototools-staging".to_string()),
        );
        let database = PathBuf::from(
            std::env::var("DATABASE_PATH").unwrap_or_else(|_| "/tmp/phototools.db".to_string()),
        );

        Ok(Self {
            roots,
            staging_dir,
            thresholds: Thresholds::default(),
            database,
        })
    }

    /// G6. Canonicalise and reject anything outside `roots`.
    pub fn resolve(&self, requested: &Path) -> Result<PathBuf, Error> {
        let canonical = requested.canonicalize().map_err(|_| {
            Error::AccessDenied(format!(
                "Path does not exist or cannot be canonicalized: {}",
                requested.display()
            ))
        })?;

        for root in &self.roots {
            if canonical.starts_with(root) {
                return Ok(canonical);
            }
        }
        Err(Error::AccessDenied(format!(
            "Path resolves outside allowed roots: {}",
            canonical.display()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::symlink;
    use tempfile::tempdir;

    #[test]
    fn test_resolve_g6() {
        let temp = tempdir().unwrap();
        let root = temp.path().join("root");
        fs::create_dir(&root).unwrap();

        let outside = temp.path().join("outside");
        fs::create_dir(&outside).unwrap();

        let inside = root.join("inside.txt");
        fs::write(&inside, "data").unwrap();

        let outside_file = outside.join("outside.txt");
        fs::write(&outside_file, "data").unwrap();

        let symlink_path = root.join("symlink_to_outside");
        symlink(&outside_file, &symlink_path).unwrap();

        let config = Config {
            roots: vec![root.canonicalize().unwrap()],
            staging_dir: PathBuf::new(),
            thresholds: Thresholds::default(),
            database: PathBuf::new(),
        };

        // 1. Path inside a root
        assert!(config.resolve(&inside).is_ok());

        // 2. Path outside a root
        assert!(config.resolve(&outside_file).is_err());

        // 3. .. traversal
        let traversal = root.join("..").join("outside").join("outside.txt");
        assert!(config.resolve(&traversal).is_err());

        // 4. Absolute path outside
        assert!(config
            .resolve(&outside_file.canonicalize().unwrap())
            .is_err());

        // 5. Symlink pointing outside
        assert!(config.resolve(&symlink_path).is_err());
    }
}
