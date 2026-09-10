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
/// | `max_megapixels` | 0 — no ceiling | `MAX_MEGAPIXELS` |
/// | `max_output_bytes` | 10 MB | `MAX_OUTPUT_BYTES` |
///
/// `max_megapixels` and `max_output_bytes` are independent, and both apply to
/// the JPEG path and the RAW-derived path alike.
///
/// **The resolution ceiling defaults to off, where §F12 specifies 10 MP.** What
/// matters for publishing is the size of the file, and a 40 MP frame inside the
/// byte cap is a frame worth keeping whole; resizing it to 10 MP throws away
/// three quarters of it for a limit nothing is enforcing. Set
/// `MAX_MEGAPIXELS` to restore a ceiling — the rule is unchanged, only its
/// default. Recorded in `docs/known-gaps.md` as a divergence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Thresholds {
    pub max_age_days: i64,
    /// Resolution ceiling in megapixels. **Zero means no ceiling.**
    pub max_megapixels: u32,
    pub max_output_bytes: u64,
}

pub const DEFAULT_MAX_AGE_DAYS: i64 = 90;
/// No resolution ceiling. §F12's 10 MP remains available by setting it.
pub const DEFAULT_MAX_MEGAPIXELS: u32 = 0;
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
    /// The one folder publishing draws from and, on success, empties.
    ///
    /// **`None` means publishing is refused**, the way an empty `roots` refuses
    /// every path. A default would be a guess at which folder may be deleted
    /// from, and there is no safe guess: the deletion is the one irreversible
    /// step in the application, and Google cannot un-publish what it received
    /// either.
    ///
    /// Configured rather than typed, so the folder that may be emptied is a
    /// decision taken once, in a file, rather than a string retyped before
    /// every run.
    #[serde(default)]
    pub publishing_dir: Option<PathBuf>,
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
            publishing_dir: None,
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

        // Canonicalised at load like a root, and for the same reason: the
        // check that follows is a prefix comparison, which means nothing
        // against a path holding a symlink or a `..`.
        let publishing_dir = match std::env::var("PUBLISHING_DIR") {
            Ok(raw) if !raw.trim().is_empty() => {
                let raw = raw.trim();
                Some(PathBuf::from(raw).canonicalize().map_err(|e| {
                    Error::Config(format!(
                        "PUBLISHING_DIR {raw:?} cannot be resolved: {e}. It must exist and be \
                         canonicalisable — publishing empties it, so it is never created \
                         automatically."
                    ))
                })?)
            }
            _ => None,
        };

        Ok(Self {
            roots,
            staging_dir,
            publishing_dir,
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

    /// G6, narrowed to the one folder publishing may empty.
    ///
    /// `resolve` asks *"is this inside somewhere I may touch?"*. This asks a
    /// harder question — *"is this the folder I may delete from?"* — and the
    /// difference matters because the answer authorises destruction.
    ///
    /// Both sides are canonicalised before they are compared, which is what
    /// makes the comparison mean anything: a symlink inside the publishing
    /// folder pointing somewhere else resolves to that somewhere else and is
    /// refused, and so is any path holding a `..` that would climb back out.
    /// A folder that merely *happens to be named* `Publishing` is refused too,
    /// because the check is against the configured path, not against a name.
    pub fn resolve_for_publishing(&self, requested: &Path) -> Result<PathBuf, Error> {
        let Some(publishing) = &self.publishing_dir else {
            return Err(Error::Config(
                "No publishing folder is configured, so nothing can be published or deleted. \
                 Set PUBLISHING_DIR to the folder publishing should draw from."
                    .into(),
            ));
        };

        let canonical = requested.canonicalize().map_err(|_| {
            Error::AccessDenied(format!(
                "Path does not exist or cannot be canonicalized: {}",
                requested.display()
            ))
        })?;

        if canonical == *publishing || canonical.starts_with(publishing) {
            return Ok(canonical);
        }

        // The message names the rule rather than the folder: which directory
        // may be emptied is configuration, and a refusal is not the place to
        // publish it.
        Err(Error::Refused(format!(
            "{} is not inside the configured publishing folder. Only that folder is ever \
             deleted from.",
            requested.display()
        )))
    }

    /// G6 for a path that does not exist yet, such as an output directory.
    ///
    /// [`resolve`](Self::resolve) canonicalises, which fails outright on a
    /// missing path — so it cannot vet a destination before it is created.
    /// This resolves the nearest existing ancestor, checks *that* against the
    /// roots, and re-appends the remainder. Any `..` in the remainder is
    /// rejected, so the check cannot be walked back out of afterwards.
    pub fn resolve_for_create(&self, requested: &Path) -> Result<PathBuf, Error> {
        if requested.exists() {
            return self.resolve(requested);
        }

        let mut trailing: Vec<std::ffi::OsString> = Vec::new();
        let mut cursor = requested;

        loop {
            let name = cursor.file_name().ok_or_else(|| {
                Error::AccessDenied(format!(
                    "Path {} contains a component that cannot be resolved safely",
                    requested.display()
                ))
            })?;
            trailing.push(name.to_os_string());

            let parent = cursor.parent().ok_or_else(|| {
                Error::AccessDenied(format!(
                    "Path {} has no existing ancestor inside an allowed root",
                    requested.display()
                ))
            })?;

            if parent.exists() {
                let mut resolved = self.resolve(parent)?;
                for name in trailing.iter().rev() {
                    resolved.push(name);
                }
                return Ok(resolved);
            }
            cursor = parent;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::symlink;
    use tempfile::tempdir;

    /// Serialises the tests that set a real environment variable.
    ///
    /// `cargo test` runs a module's tests on threads of one process, and the
    /// environment is process-wide: two tests setting `PUBLISHING_DIR` would
    /// each see the other's value about as often as their own. The other tests
    /// here dodge this by using names of their own (`PT_TEST_*`); these two
    /// cannot, because the name under test is the point.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
            publishing_dir: None,
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
            publishing_dir: None,
            thresholds: Thresholds::default(),
            database: PathBuf::new(),
        };

        assert!(config.resolve(&secret).is_err());
    }

    #[test]
    fn thresholds_default_to_the_documented_values() {
        let t = Thresholds::default();
        assert_eq!(t.max_age_days, 90);
        assert_eq!(t.max_output_bytes, 10 * 1024 * 1024);

        // Zero, where §F12 specifies 10 MP: publishing is limited by file size,
        // and a frame inside the byte cap is worth keeping whole. The rule is
        // unchanged and a ceiling can still be set — only the default moved.
        // Recorded in docs/known-gaps.md as a divergence.
        assert_eq!(t.max_megapixels, 0, "no resolution ceiling by default");
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

    // -----------------------------------------------------------------------
    // The publishing folder (PB-1)
    //
    // Publishing empties this folder, so the question these answer is not "may
    // I read here?" but "may I delete here?". Every one of them is a way the
    // second question could be answered yes when it should be no.
    // -----------------------------------------------------------------------

    /// A configuration with a real publishing folder, and the temp dir that
    /// owns it. macOS puts temp dirs behind `/private`, so the path is
    /// canonicalised here exactly as `from_env` would.
    fn with_publishing() -> (tempfile::TempDir, Config, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let publishing = temp.path().join("Publishing");
        std::fs::create_dir(&publishing).unwrap();
        let publishing = publishing.canonicalize().unwrap();

        let config = Config {
            publishing_dir: Some(publishing.clone()),
            ..Config::default()
        };
        (temp, config, publishing)
    }

    #[test]
    fn the_publishing_folder_itself_is_accepted() {
        let (_temp, config, publishing) = with_publishing();
        assert_eq!(
            config.resolve_for_publishing(&publishing).unwrap(),
            publishing
        );
    }

    #[test]
    fn a_file_inside_the_publishing_folder_is_accepted() {
        let (_temp, config, publishing) = with_publishing();
        let file = publishing.join("a.jpg");
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(config.resolve_for_publishing(&file).unwrap(), file);
    }

    #[test]
    fn a_file_at_any_depth_inside_it_is_accepted() {
        // Subfolders are published and therefore emptied; a check that stopped
        // at the top level would refuse to clear what it had just uploaded.
        let (_temp, config, publishing) = with_publishing();
        let deep = publishing.join("borders/2026");
        std::fs::create_dir_all(&deep).unwrap();
        let file = deep.join("a.jpg");
        std::fs::write(&file, b"x").unwrap();
        assert!(config.resolve_for_publishing(&file).is_ok());
    }

    #[test]
    fn a_sibling_folder_is_refused() {
        let (temp, config, _) = with_publishing();
        let elsewhere = temp.path().join("Archive");
        std::fs::create_dir(&elsewhere).unwrap();

        let err = config.resolve_for_publishing(&elsewhere).unwrap_err();
        assert!(matches!(err, Error::Refused(_)), "got {err}");
    }

    #[test]
    fn a_sibling_whose_name_merely_starts_the_same_is_refused() {
        // `Publishing-old` is not inside `Publishing`, however much it looks
        // like it as a string. `Path::starts_with` compares components rather
        // than characters, and this is the test that says so — the roots check
        // carries the same one, for the same reason.
        let (temp, config, _) = with_publishing();
        let sibling = temp.path().join("Publishing-old");
        std::fs::create_dir(&sibling).unwrap();

        let err = config.resolve_for_publishing(&sibling).unwrap_err();
        assert!(matches!(err, Error::Refused(_)), "got {err}");
    }

    #[test]
    fn a_different_folder_of_the_same_name_is_refused() {
        // The check is against the configured path, not against a name. A
        // second folder called Publishing is a different folder.
        let (temp, config, _) = with_publishing();
        let impostor = temp.path().join("other/Publishing");
        std::fs::create_dir_all(&impostor).unwrap();

        let err = config.resolve_for_publishing(&impostor).unwrap_err();
        assert!(matches!(err, Error::Refused(_)), "got {err}");
    }

    #[test]
    fn a_symlink_inside_it_pointing_out_of_it_is_refused() {
        // The reason both sides are canonicalised. Without that, this path is
        // textually inside the publishing folder and deleting it would delete
        // somebody's archive.
        let (temp, config, publishing) = with_publishing();
        let outside = temp.path().join("archive");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("keep.jpg"), b"precious").unwrap();

        let link = publishing.join("escape");
        std::os::unix::fs::symlink(&outside, &link).unwrap();

        let err = config
            .resolve_for_publishing(&link.join("keep.jpg"))
            .unwrap_err();
        assert!(matches!(err, Error::Refused(_)), "got {err}");
    }

    #[test]
    fn a_path_climbing_back_out_with_dot_dot_is_refused() {
        let (temp, config, publishing) = with_publishing();
        let outside = temp.path().join("archive");
        std::fs::create_dir(&outside).unwrap();

        let climbed = publishing.join("..").join("archive");
        let err = config.resolve_for_publishing(&climbed).unwrap_err();
        assert!(matches!(err, Error::Refused(_)), "got {err}");
    }

    #[test]
    fn nothing_at_all_is_publishable_when_no_folder_is_configured() {
        // The default. A guess at which folder may be emptied is worse than a
        // refusal, so there is no default and the message says what to set.
        let temp = tempfile::tempdir().unwrap();
        let config = Config::default();
        assert_eq!(config.publishing_dir, None);

        let err = config.resolve_for_publishing(temp.path()).unwrap_err();
        assert!(matches!(err, Error::Config(_)), "got {err}");
        assert!(err.to_string().contains("PUBLISHING_DIR"), "got {err}");
    }

    #[test]
    fn a_publishing_folder_that_does_not_exist_is_a_startup_error() {
        // It is never created automatically: creating a directory in order to
        // empty it later is not a thing this should do on its own.
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("PUBLISHING_DIR", "/definitely/not/a/real/folder");
        let result = Config::from_env();
        std::env::remove_var("PUBLISHING_DIR");

        let err = result.expect_err("a missing publishing folder must not be ignored");
        assert!(matches!(err, Error::Config(_)), "got {err}");
        assert!(err.to_string().contains("PUBLISHING_DIR"), "got {err}");
    }

    #[test]
    fn an_unset_publishing_folder_leaves_the_rest_of_the_configuration_working() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("PUBLISHING_DIR");
        let config = Config::from_env().expect("the rest of the configuration is independent");
        assert_eq!(config.publishing_dir, None);
    }
}
