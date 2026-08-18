//! Phase 7 acceptance, minus the parts that need macOS.
//!
//! The acceptance criteria are "the app launches and runs an F1 date scan on a
//! local folder" and "with the server stopped, the app still starts and local
//! tools work". Launching a Tauri window needs a Mac and a display; what is
//! testable here is that the work those commands delegate to succeeds with no
//! server present, and that the server connection reports rather than fails.

use phototools_core::config::{Config, Thresholds};
use phototools_core::jobs::{JobRunner, JobStatus, NoEvents};
use phototools_core::ledger::Ledger;
use phototools_core::tools::f1_dates;
use phototools_desktop::server::{ServerConnection, ServerSettings};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

struct Fixture {
    _temp: tempfile::TempDir,
    config: Config,
    root: PathBuf,
}

fn fixture() -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("library");
    std::fs::create_dir(&root).unwrap();

    let config = Config {
        roots: vec![root.canonicalize().unwrap()],
        staging_dir: temp.path().join("staging"),
        thresholds: Thresholds::default(),
        database: temp.path().join("ledger.sqlite3"),
    };
    let root = root.canonicalize().unwrap();
    Fixture {
        _temp: temp,
        config,
        root,
    }
}

/// **Acceptance:** an F1 date scan runs on a local folder, with no server.
#[test]
fn a_date_scan_runs_locally_with_no_server_present() {
    let f = fixture();
    for name in ["a.jpg", "b.jpg", "notes.txt"] {
        std::fs::write(f.root.join(name), b"x").unwrap();
    }

    let resolved = f.config.resolve(&f.root).unwrap();
    let results = f1_dates::scan_dates(&resolved, false).unwrap();

    // Only the media files, and each classified.
    assert_eq!(results.len(), 2);
    for result in &results {
        assert_eq!(result.status, f1_dates::DateStatus::MissingMetadata);
        assert!(result.fs_date_source.is_some());
    }
}

/// **Acceptance:** with the server stopped, local work still succeeds.
#[tokio::test]
async fn local_jobs_run_while_the_server_is_unreachable() {
    let f = fixture();

    // Port 1 is not listening, which is the "NAS is off" case.
    let connection = ServerConnection::new(ServerSettings {
        base_url: "http://127.0.0.1:1".into(),
        auth_token: None,
    });
    let status = connection.status().await;
    assert!(!status.reachable, "precondition: no server");
    assert!(status.detail.is_some(), "the UI needs a reason to show");

    // A local job runs regardless.
    let ledger = Ledger::open(&f.config.database).unwrap();
    let runner = JobRunner::new(ledger, Arc::new(NoEvents));

    let root = f.root.clone();
    let id = runner
        .spawn("dates_scan", 0, move |progress| {
            let results = f1_dates::scan_dates(&root, false)?;
            progress.report(results.len() as u64, results.len() as u64, "scanned");
            Ok(format!("{} files scanned", results.len()))
        })
        .unwrap();

    let mut job = runner.get(&id).unwrap().unwrap();
    for _ in 0..200 {
        if job.status.is_terminal() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
        job = runner.get(&id).unwrap().unwrap();
    }

    assert_eq!(
        job.status,
        JobStatus::Completed,
        "a local job must not depend on the server: {:?}",
        job.error
    );
}

/// G6 holds on the desktop too: the command layer resolves against the roots.
#[test]
fn a_path_outside_the_roots_is_refused() {
    let f = fixture();

    let outside = f.root.parent().unwrap().join("outside");
    std::fs::create_dir_all(&outside).unwrap();

    assert!(f.config.resolve(&outside).is_err());
    assert!(f.config.resolve(&f.root.join("..")).is_err());
    assert!(f.config.resolve(&f.root).is_ok());
}

/// The server address is settable at runtime, which is what the settings pane
/// changes (task 4).
#[tokio::test]
async fn the_server_address_can_be_changed_without_a_restart() {
    let connection = ServerConnection::new(ServerSettings::default());
    assert_eq!(connection.settings().base_url, "http://127.0.0.1:3000");

    connection.set_settings(ServerSettings {
        base_url: "http://nas.local:3000".into(),
        auth_token: None,
    });
    assert_eq!(connection.settings().base_url, "http://nas.local:3000");

    // And the probe uses the new address.
    let status = connection.status().await;
    assert_eq!(status.base_url, "http://nas.local:3000");
}
