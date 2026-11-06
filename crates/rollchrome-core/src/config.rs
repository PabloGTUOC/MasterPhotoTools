//! Rollchrome configuration helpers.
//!
//! Rollchrome reads PhotoTools' configuration file (`Config::load()`) so its roots,
//! publishing folder, LUT library, presets and mask models work without copying anything
//! (decision A), while keeping its jobs in a ledger of its own (decision B).

use phototools_core::config::Config;
use phototools_core::dirs;
use std::path::PathBuf;

/// Returns the path to Rollchrome's job ledger (`rollchrome.sqlite3`), beside PhotoTools'
/// `db.sqlite3`.
///
/// At start-up each application marks every job still recorded as running as *interrupted*
/// to recover from crashes (F17). Using a separate ledger ensures starting Rollchrome while
/// PhotoTools is running a job never marks PhotoTools' live job as interrupted.
pub fn ledger_path(config: &Config) -> PathBuf {
    config.database.with_file_name("rollchrome.sqlite3")
}

/// Directory where the managed 3D LUT library is stored (ED-6).
///
/// Stored alongside the application database in the app data directory
/// (e.g. `~/Library/Application Support/masterphototools/luts` on macOS).
pub fn lut_dir(config: &Config) -> PathBuf {
    config
        .database
        .parent()
        .map(|p| p.join("luts"))
        .unwrap_or_else(|| {
            dirs::data_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("masterphototools/luts")
        })
}

/// Directory where the automatic-mask models are kept once downloaded (ED-23).
///
/// Beside the database, like the LUTs and presets; never bundled with the
/// application, since the two models are 400 MB together.
pub fn models_dir(config: &Config) -> PathBuf {
    config
        .database
        .parent()
        .map(|p| p.join("models"))
        .unwrap_or_else(|| {
            dirs::data_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("masterphototools/models")
        })
}

/// Directory where presets are stored (ED-17).
///
/// Stored alongside the application database in the app data directory
/// (e.g. `~/Library/Application Support/masterphototools/presets` on macOS).
pub fn presets_dir(config: &Config) -> PathBuf {
    config
        .database
        .parent()
        .map(|p| p.join("presets"))
        .unwrap_or_else(|| {
            dirs::data_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("masterphototools/presets")
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use phototools_core::config::Thresholds;

    #[test]
    fn rollchrome_keeps_its_jobs_in_a_ledger_of_its_own_beside_phototools() {
        let dir = PathBuf::from("/tmp/phototools-test");
        let config = Config {
            roots: vec![dir.join("library")],
            staging_dir: dir.join("staging"),
            publishing_dir: None,
            thresholds: Thresholds::default(),
            database: dir.join("db.sqlite3"),
        };

        let path = ledger_path(&config);
        assert_eq!(path, dir.join("rollchrome.sqlite3"));
        assert_ne!(path, config.database);
    }
}
