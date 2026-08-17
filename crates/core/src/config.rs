//! Settings, roots, thresholds

use crate::error::Error;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Validation thresholds (specification F12).
///
/// Defaults, all overridable by environment variable:
///
/// | Field | Default | Variable |
/// |---|---|---|
/// | `max_age_days` | 90 | `MAX_AGE_DAYS` |
/// | `max_megapixels` | 10 | `MAX_MEGAPIXELS` |
/// | `max_output_bytes` | 10 MB | `MAX_OUTPUT_BYTES` |
///
/// `max_megapixels` and `max_output_bytes` are independent, and both apply to
/// the JPEG path and the RAW-derived path alike.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Thresholds {
    pub max_age_days: i64,
    pub max_megapixels: u32,
    pub max_output_bytes: u64,
}

pub const DEFAULT_MAX_AGE_DAYS: i64 = 90;
pub const DEFAULT_MAX_MEGAPIXELS: u32 = 10;
pub const DEFAULT_MAX_OUTPUT_BYTES: u64 = 10 * 1024 * 1024;

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            max_age_days: DEFAULT_MAX_AGE_DAYS,
            max_megapixels: DEFAULT_MAX_MEGAPIXELS,
            max_output_bytes: DEFAULT_MAX_OUTPUT_BYTES,
        }
    }
}

/// Read `name` from the environment, falling back to `default`.
///
/// A variable that is set but unparseable is an error rather than a silent
/// fallback: a typo in `MAX_MEGAPIXELS` must not quietly restore the default
/// and let oversized frames through (specification §9.2 invariant 6).
fn env_or<T>(name: &str, default: T) -> Result<T, Error>
where
    T: std::str::FromStr,
{
    match std::env::var(name) {
        Err(_) => Ok(default),
        Ok(raw) => raw.trim().parse::<T>().map_err(|_| {
            Error::Config(format!(
                "{name} is set to {raw:?}, which is not a valid value"
            ))
        }),
    }
}

impl Thresholds {
    pub fn from_env() -> Result<Self, Error> {
        Ok(Self {
            max_age_days: env_or("MAX_AGE_DAYS", DEFAULT_MAX_AGE_DAYS)?,
            max_megapixels: env_or("MAX_MEGAPIXELS", DEFAULT_MAX_MEGAPIXELS)?,
            max_output_bytes: env_or("MAX_OUTPUT_BYTES", DEFAULT_MAX_OUTPUT_BYTES)?,
        })
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

    /// Load configuration from environment variables, using documented defaults.
    ///
    /// `ROOTS` is a colon-separated list. Each entry is canonicalised at load
    /// time; an entry that cannot be canonicalised is **rejected**, not kept
    /// verbatim. A root that is itself a symlink or a relative path would
    /// otherwise make `resolve`'s prefix check meaningless (G6).
    pub fn from_env() -> Result<Self, Error> {
        let roots_str = std::env::var("ROOTS").unwrap_or_default();
        let mut roots = Vec::new();
        for entry in roots_str.split(':').filter(|s| !s.is_empty()) {
            let canonical = PathBuf::from(entry).canonicalize().map_err(|e| {
                Error::Config(format!(
                    "ROOTS entry {entry:?} cannot be resolved: {e}. \
                     Every root must exist and be canonicalisable."
                ))
            })?;
            roots.push(canonical);
        }

        let staging_dir = PathBuf::from(
            std::env::var("STAGING_DIR").unwrap_or_else(|_| "/tmp/phototools-staging".to_string()),
        );
        let database = PathBuf::from(
            std::env::var("DATABASE_PATH").unwrap_or_else(|_| "/tmp/phototools.db".to_string()),
        );

        Ok(Self {
            roots,
            staging_dir,
            thresholds: Thresholds::from_env()?,
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

    /// A sibling directory whose name merely starts with a root's name is not
    /// inside that root. `Path::starts_with` is component-wise, so this holds —
    /// the test pins the behaviour against a future refactor to string prefixes.
    #[test]
    fn a_sibling_with_a_shared_name_prefix_is_rejected() {
        let temp = tempdir().unwrap();
        let root = temp.path().join("photos");
        let sibling = temp.path().join("photos-private");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&sibling).unwrap();

        let secret = sibling.join("secret.jpg");
        fs::write(&secret, "data").unwrap();

        let config = Config {
            roots: vec![root.canonicalize().unwrap()],
            staging_dir: PathBuf::new(),
            thresholds: Thresholds::default(),
            database: PathBuf::new(),
        };

        assert!(config.resolve(&secret).is_err());
    }

    #[test]
    fn thresholds_default_to_the_documented_values() {
        let t = Thresholds::default();
        assert_eq!(t.max_age_days, 90);
        assert_eq!(t.max_megapixels, 10);
        assert_eq!(t.max_output_bytes, 10 * 1024 * 1024);
    }

    // These use unique variable names so they cannot race with each other or
    // with any other test in the process.
    #[test]
    fn an_unset_variable_falls_back_to_its_default() {
        assert_eq!(env_or("PT_TEST_UNSET_VAR", 42u32).unwrap(), 42);
    }

    #[test]
    fn a_set_variable_is_parsed() {
        std::env::set_var("PT_TEST_PARSED_VAR", " 25 ");
        assert_eq!(env_or("PT_TEST_PARSED_VAR", 10u32).unwrap(), 25);
        std::env::remove_var("PT_TEST_PARSED_VAR");
    }

    #[test]
    fn a_malformed_variable_is_an_error_not_a_silent_default() {
        std::env::set_var("PT_TEST_BAD_VAR", "ten");
        let result = env_or("PT_TEST_BAD_VAR", 10u32);
        std::env::remove_var("PT_TEST_BAD_VAR");

        let err = result.expect_err("a typo must not quietly restore the default");
        assert!(matches!(err, Error::Config(_)));
    }
}
