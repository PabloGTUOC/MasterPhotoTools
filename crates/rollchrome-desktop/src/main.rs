// Prevents an extra console window on Windows in release builds.
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use phototools_core::config::Config;
use phototools_core::ledger::Ledger;
use rollchrome_desktop::{commands, jobs, AppState};
use std::sync::Arc;
use tauri::Manager;

fn main() {
    // The server refuses to start on a bad configuration; a window cannot, because a
    // window that never opens explains nothing. Safe defaults, and say why (G10).
    let config = Config::load().unwrap_or_else(|e| {
        eprintln!(
            "Could not load configuration from {}: {e}. Using default configuration instead.",
            Config::config_path().display()
        );
        Config::default()
    });

    let ledger_path = rollchrome_core::config::ledger_path(&config);
    let ledger = match Ledger::open(&ledger_path) {
        Ok(ledger) => ledger,
        Err(e) => {
            eprintln!(
                "Could not open the ledger at {}: {e}",
                ledger_path.display()
            );
            std::process::exit(1);
        }
    };

    tauri::Builder::default()
        .setup(move |app| {
            // The sink needs an AppHandle, which only exists once the app is
            // being set up — hence building state here rather than earlier.
            let sink = Arc::new(jobs::TauriSink::new(app.handle().clone()));
            let state = AppState::new(config.clone(), ledger, sink);

            // F17: a job orphaned by an unclean shutdown must not disappear.
            match state.jobs.recover() {
                Ok(recovered) if !recovered.is_empty() => {
                    eprintln!(
                        "marked {} interrupted job(s) after an unclean shutdown",
                        recovered.len()
                    );
                }
                Ok(_) => {}
                Err(e) => eprintln!("could not recover interrupted jobs: {e}"),
            }

            app.manage(state);
            Ok(())
        })
        .on_window_event(|window, event| {
            // Close preview session when the window closes so the last session's
            // ~65 MB does not remain allocated while the macOS app continues running.
            if matches!(
                event,
                tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed
            ) {
                if let Some(state) = window.try_state::<AppState>() {
                    state.close_any_preview();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_job,
            commands::cancel_job,
            commands::list_directory,
            commands::list_roots,
            commands::load_recipe,
            commands::save_recipe,
            commands::export_edited_image,
            commands::plan_bulk_lut,
            commands::apply_bulk_lut,
            commands::open_preview,
            commands::render_preview,
            commands::render_mask_coverage,
            commands::auto_mask_models,
            commands::download_mask_model,
            commands::make_auto_mask,
            commands::close_preview,
            commands::list_luts,
            commands::import_lut,
            commands::list_presets,
            commands::load_preset,
            commands::save_preset,
            commands::rename_preset,
            commands::delete_preset,
            commands::plan_bulk_edit,
            commands::apply_bulk_edit,
            commands::plan_develop,
            commands::apply_develop,
            commands::list_builtin_looks,
            commands::list_develop_folder,
            commands::develop_thumbnail,
            commands::preview_develop,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the Rollchrome desktop application");
}
