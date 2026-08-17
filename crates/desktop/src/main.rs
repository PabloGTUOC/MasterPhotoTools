// Prevents an extra console window on Windows in release builds.
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use phototools_core::config::Config;
use phototools_core::ledger::Ledger;
use phototools_desktop::{commands, jobs, AppState};
use std::sync::Arc;
use tauri::Manager;

fn main() {
    let config = Config::load().unwrap_or_else(|_| Config::default());

    let ledger = match Ledger::open(&config.database) {
        Ok(ledger) => ledger,
        Err(e) => {
            eprintln!(
                "Could not open the ledger at {}: {e}",
                config.database.display()
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
        .invoke_handler(tauri::generate_handler![
            commands::bootstrap,
            commands::get_config,
            commands::save_config,
            commands::get_server_settings,
            commands::set_server_settings,
            commands::server_status,
            commands::get_job,
            commands::list_directory,
            commands::scan_dates,
            commands::fix_dates,
            commands::plan_rename,
            commands::apply_rename,
            commands::split,
            commands::border,
            commands::tiff_to_jpeg,
            commands::contact_sheet,
            commands::transform,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the PhotoTools desktop application");
}
