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
        publishing_dir: None,
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

// ---------------------------------------------------------------------------
// Telling "nothing is there" from "something else is there"
// ---------------------------------------------------------------------------

/// A one-shot listener that answers every request with the given status.
///
/// Stands in for the case that actually happens: another application already
/// on the port the desktop is pointed at.
async fn wrong_service(status: &'static str, body: &'static str) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let mut buffer = [0_u8; 1024];
            let _ = socket.read(&mut buffer).await;
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(response.as_bytes()).await;
        }
    });

    format!("http://127.0.0.1:{port}")
}

#[tokio::test]
async fn a_wrong_address_is_reported_differently_from_a_stopped_server() {
    // Both are "not reachable", and the fix is opposite: one is a stopped
    // server, the other is a right-running server at the wrong address. A
    // message that says "offline" for both sends somebody to restart a service
    // that is already running.
    let elsewhere = wrong_service("404 Not Found", r#"{"error":"Not found"}"#).await;

    let answered = ServerConnection::new(ServerSettings {
        base_url: elsewhere.clone(),
        auth_token: None,
    })
    .status()
    .await;

    let silent = ServerConnection::new(ServerSettings {
        base_url: "http://127.0.0.1:1".into(),
        auth_token: None,
    })
    .status()
    .await;

    assert!(!answered.reachable);
    assert!(!silent.reachable);

    let answered_detail = answered.detail.unwrap();
    let silent_detail = silent.detail.unwrap();

    assert!(
        answered_detail.contains("listening"),
        "a service that answered must not read as absent: {answered_detail}"
    );
    assert!(
        answered_detail.contains("404"),
        "the status it gave back is the clue to the wrong port: {answered_detail}"
    );
    assert!(
        silent_detail.contains("Nothing answered"),
        "an empty port must read as nothing there: {silent_detail}"
    );
    assert_ne!(answered_detail, silent_detail);
}

/// Something on the port that answers `200` but is not PhotoTools.
#[tokio::test]
async fn a_two_hundred_from_the_wrong_service_is_not_reachable() {
    let elsewhere = wrong_service("200 OK", r#"{"hello":"i am something else"}"#).await;

    let status = ServerConnection::new(ServerSettings {
        base_url: elsewhere,
        auth_token: None,
    })
    .status()
    .await;

    assert!(
        !status.reachable,
        "a 200 that is not the health document must not count as the server being up"
    );
    assert!(status.detail.unwrap().contains("listening"));
}

/// Helper to create a valid 2x2x2 cube LUT on disk.
fn create_test_cube(dir: &std::path::Path) -> PathBuf {
    let p = dir.join("test.cube");
    let content = "\
TITLE \"Test Cube\"
LUT_3D_SIZE 2
0.0 0.0 0.0
1.0 0.0 0.0
0.0 1.0 0.0
1.0 1.0 0.0
0.0 0.0 1.0
1.0 0.0 1.0
0.0 1.0 1.0
1.0 1.0 1.0
";
    std::fs::write(&p, content).unwrap();
    p
}

#[test]
fn export_edited_image_refuses_destination_inside_publishing_folder() {
    let mut f = fixture();
    let pub_dir = f._temp.path().join("publishing");
    std::fs::create_dir_all(&pub_dir).unwrap();
    f.config.publishing_dir = Some(pub_dir.canonicalize().unwrap());

    let ledger = Ledger::open(&f.config.database).unwrap();
    let state = phototools_desktop::AppState::new(f.config.clone(), ledger, Arc::new(NoEvents));

    let img_path = f.root.join("source.jpg");
    image::RgbImage::new(60, 40).save(&img_path).unwrap();

    let recipe = phototools_core::media::edit::AdjustmentRecipe::default();
    let err = phototools_desktop::commands::edit::export_edited_image_impl(
        &state,
        img_path.to_string_lossy().to_string(),
        recipe,
        pub_dir.to_string_lossy().to_string(),
    )
    .unwrap_err();

    assert!(
        err.contains("publishing folder"),
        "must refuse writing inside Publishing folder (MV-16.7); got: {err}"
    );
}

#[test]
fn bulk_lut_refuses_output_directory_inside_publishing_folder() {
    let mut f = fixture();
    let pub_dir = f._temp.path().join("publishing");
    std::fs::create_dir_all(&pub_dir).unwrap();
    f.config.publishing_dir = Some(pub_dir.canonicalize().unwrap());

    let ledger = Ledger::open(&f.config.database).unwrap();
    let state = phototools_desktop::AppState::new(f.config.clone(), ledger, Arc::new(NoEvents));

    let img_path = f.root.join("photo.jpg");
    image::RgbImage::new(60, 40).save(&img_path).unwrap();

    // Import a test LUT into the managed library
    let cube = create_test_cube(&f.root);
    phototools_desktop::commands::edit::import_lut_impl(&state, cube.to_string_lossy().to_string())
        .unwrap();

    let err = phototools_desktop::commands::edit::plan_bulk_lut_impl(
        &state,
        vec![img_path.to_string_lossy().to_string()],
        "test.cube".into(),
        1.0,
        pub_dir.to_string_lossy().to_string(),
        false,
    )
    .unwrap_err();

    assert!(
        err.contains("publishing folder"),
        "must refuse output inside Publishing folder (MV-16.7); got: {err}"
    );
}

#[test]
fn apply_bulk_lut_refuses_when_the_lut_changed_after_the_reviewed_plan() {
    let f = fixture();
    let ledger = Ledger::open(&f.config.database).unwrap();
    let state = phototools_desktop::AppState::new(f.config.clone(), ledger, Arc::new(NoEvents));

    let img_path = f.root.join("source.jpg");
    image::RgbImage::new(60, 40).save(&img_path).unwrap();

    let cube = create_test_cube(&f.root);
    phototools_desktop::commands::edit::import_lut_impl(&state, cube.to_string_lossy().to_string())
        .unwrap();

    let out_dir = f.root.join("graded_out");
    std::fs::create_dir_all(&out_dir).unwrap();

    // 1. User plans and reviews the dry run
    let plan = phototools_desktop::commands::edit::plan_bulk_lut_impl(
        &state,
        vec![img_path.to_string_lossy().to_string()],
        "test.cube".into(),
        1.0,
        out_dir.to_string_lossy().to_string(),
        false,
    )
    .unwrap();
    let reviewed_lut_sha256 = plan.lut_sha256;

    // 2. The LUT file on disk is modified after review
    let lut_in_lib = f.config.lut_dir().join("test.cube");
    let modified_content = "\
TITLE \"Modified Test Cube\"
LUT_3D_SIZE 2
0.2 0.2 0.2
1.0 0.0 0.0
0.0 1.0 0.0
1.0 1.0 0.0
0.0 0.0 1.0
1.0 0.0 1.0
0.0 1.0 1.0
1.0 1.0 1.0
";
    std::fs::write(&lut_in_lib, modified_content).unwrap();

    // 3. User clicks Run with the reviewed hash
    let err = phototools_desktop::commands::edit::apply_bulk_lut_impl(
        &state,
        vec![img_path.to_string_lossy().to_string()],
        "test.cube".into(),
        1.0,
        out_dir.to_string_lossy().to_string(),
        reviewed_lut_sha256,
        false,
    )
    .unwrap_err();

    assert!(
        err.contains("changed on disk since the reviewed plan"),
        "must refuse execution when LUT changed after reviewed plan; got: {err}"
    );
}

#[test]
fn cancel_job_stops_an_active_job_and_refuses_a_finished_job() {
    let f = fixture();
    let ledger = Ledger::open(&f.config.database).unwrap();
    let state = phototools_desktop::AppState::new(f.config.clone(), ledger, Arc::new(NoEvents));

    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (done_tx, done_rx) = std::sync::mpsc::channel();

    let job_id = state
        .jobs
        .spawn("long_job", 10, move |p| {
            started_tx.send(()).unwrap();
            while !p.cancelled() {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            done_tx.send(()).unwrap();
            Ok("stopped early".to_string())
        })
        .unwrap();

    // Guarantee the worker thread is actively executing its loop
    started_rx.recv().unwrap();

    // The desktop command reaches the running job
    assert!(phototools_desktop::commands::cancel_job_impl(&state, job_id.clone()).unwrap());
    done_rx.recv().unwrap();

    // Wait for the background worker thread to terminate and update the ledger
    let mut finished_job = None;
    for _ in 0..100 {
        if let Some(job) = state.jobs.get(&job_id).unwrap() {
            if job.status.is_terminal() {
                finished_job = Some(job);
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    let job = finished_job.expect("job must complete and reach terminal status");
    assert_eq!(job.status, phototools_core::jobs::JobStatus::Cancelled);
    assert_eq!(job.summary.as_deref(), Some("stopped early"));

    // Cancelling a finished job returns false
    assert!(!phototools_desktop::commands::cancel_job_impl(&state, job_id).unwrap());

    // Cancelling an unknown job returns false
    assert!(!phototools_desktop::commands::cancel_job_impl(&state, "unknown_job".into()).unwrap());
}

#[test]
fn render_preview_returns_width_height_and_rgba_of_that_size() {
    let f = fixture();
    let ledger = Ledger::open(&f.config.database).unwrap();
    let state = phototools_desktop::AppState::new(f.config.clone(), ledger, Arc::new(NoEvents));

    let img_path = f.root.join("photo.jpg");
    image::RgbImage::new(60, 40).save(&img_path).unwrap();

    let open_res = phototools_desktop::commands::edit::open_preview_impl(
        &state,
        img_path.to_string_lossy().to_string(),
    )
    .unwrap();

    assert_eq!(open_res.drag, (60, 40));
    assert_eq!(open_res.settle, (60, 40));
    assert_eq!(open_res.orientation, 1);
    assert!(!open_res.read_only);

    let bytes = phototools_desktop::commands::edit::render_preview_impl(
        &state,
        open_res.session_id,
        phototools_core::media::edit::AdjustmentRecipe::default(),
        Some(phototools_core::media::edit::preview::PreviewStage::Drag),
    )
    .unwrap();

    let width = u32::from_be_bytes(bytes[0..4].try_into().unwrap());
    let height = u32::from_be_bytes(bytes[4..8].try_into().unwrap());
    let orientation = u32::from_be_bytes(bytes[8..12].try_into().unwrap());
    assert_eq!(width, 60);
    assert_eq!(height, 40);
    assert_eq!(orientation, 1);
    assert_eq!(
        bytes.len(),
        phototools_core::media::edit::PREVIEW_FRAME_HEADER_LEN + (60 * 40 * 4),
        "payload must contain the header and exactly width*height*4 RGBA bytes"
    );
    // A black frame: every pixel is counted, and every one is shadow-clipped.
    let pixels = u32::from_be_bytes(bytes[12..16].try_into().unwrap());
    let shadow_clipped = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    assert_eq!(pixels, 60 * 40);
    assert_eq!(shadow_clipped, 60 * 40);
}

#[test]
fn opening_a_second_preview_closes_the_first() {
    let f = fixture();
    let ledger = Ledger::open(&f.config.database).unwrap();
    let state = phototools_desktop::AppState::new(f.config.clone(), ledger, Arc::new(NoEvents));

    let img1 = f.root.join("first.jpg");
    image::RgbImage::new(40, 40).save(&img1).unwrap();
    let img2 = f.root.join("second.jpg");
    image::RgbImage::new(80, 80).save(&img2).unwrap();

    let session1 = phototools_desktop::commands::edit::open_preview_impl(
        &state,
        img1.to_string_lossy().to_string(),
    )
    .unwrap()
    .session_id;

    let session2 = phototools_desktop::commands::edit::open_preview_impl(
        &state,
        img2.to_string_lossy().to_string(),
    )
    .unwrap()
    .session_id;

    assert_ne!(session1, session2);

    // Session 1 is now closed and must be refused
    let err = phototools_desktop::commands::edit::render_preview_impl(
        &state,
        session1,
        phototools_core::media::edit::AdjustmentRecipe::default(),
        None,
    )
    .unwrap_err();
    assert!(
        err.contains("closed or replaced"),
        "session 1 must be closed after opening session 2; got: {err}"
    );

    // Session 2 is active and succeeds
    let res = phototools_desktop::commands::edit::render_preview_impl(
        &state,
        session2,
        phototools_core::media::edit::AdjustmentRecipe::default(),
        None,
    );
    assert!(res.is_ok(), "session 2 must be active and renderable");
}

#[test]
fn a_saved_recipe_is_loaded_again_when_the_photo_is_reopened() {
    let f = fixture();
    let ledger = Ledger::open(&f.config.database).unwrap();
    let state = phototools_desktop::AppState::new(f.config.clone(), ledger, Arc::new(NoEvents));

    let img = f.root.join("reopen.jpg");
    image::RgbImage::new(40, 40).save(&img).unwrap();

    let recipe = phototools_core::media::edit::AdjustmentRecipe {
        exposure: 1.5,
        temperature: -10.0,
        contrast: 25.0,
        ..Default::default()
    };

    let path_str = img.to_string_lossy().to_string();
    phototools_desktop::commands::edit::save_recipe_impl(&state, path_str.clone(), recipe).unwrap();

    // Reopen and load
    let loaded = phototools_desktop::commands::edit::load_recipe_impl(&state, path_str)
        .unwrap()
        .expect("saved recipe must be loaded");
    assert_eq!(loaded.exposure, 1.5);
    assert_eq!(loaded.temperature, -10.0);
    assert_eq!(loaded.contrast, 25.0);
}

#[test]
fn an_unreadable_sidecar_is_reported_not_treated_as_no_edits() {
    let f = fixture();
    let ledger = Ledger::open(&f.config.database).unwrap();
    let state = phototools_desktop::AppState::new(f.config.clone(), ledger, Arc::new(NoEvents));

    let img = f.root.join("corrupt.jpg");
    image::RgbImage::new(40, 40).save(&img).unwrap();

    // Write invalid JSON to companion sidecar
    let sidecar = f.root.join("corrupt.jpg.photoedit");
    std::fs::write(&sidecar, b"not valid json").unwrap();

    let path_str = img.to_string_lossy().to_string();
    let err = phototools_desktop::commands::edit::load_recipe_impl(&state, path_str)
        .expect_err("unreadable sidecar must error, never return None");
    assert!(
        err.contains("JSON") || err.contains("syntax") || err.contains("corrupt"),
        "error must report the corrupted sidecar; got: {err}"
    );
}
