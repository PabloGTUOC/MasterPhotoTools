//! Rollchrome configuration helpers.
//!
//! Rollchrome reads PhotoTools' configuration file (`Config::load()`) so its roots,
//! publishing folder, LUT library, presets and mask models work without copying anything
//! (decision A), while keeping its jobs in a ledger of its own (decision B).

use phototools_core::config::Config;
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
