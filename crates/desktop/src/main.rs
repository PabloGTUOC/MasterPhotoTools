// Prevents an extra console window on Windows in release builds.
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use phototools_core::config::Config;
use phototools_core::ledger::Ledger;
use phototools_desktop::detection::{self, CardDetected};
use phototools_desktop::{commands, jobs, AppState};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};

/// The Tauri event a detected card arrives on.
const CARD_EVENT: &str = "phototools://card-detected";

/// What happens when a card is detected: a native notification (F10), and an
/// event so the UI can offer to review it.
fn notify_card(handle: AppHandle) -> impl Fn(CardDetected) + Send + 'static {
    move |card: CardDetected| {
        use tauri_plugin_notification::NotificationExt;

        // F10's notification: `EOS_DIGITAL — 412 new shots. Review?`
        if let Err(e) = handle
            .notification()
            .builder()
            .title("Card detected")
            .body(card.notification())
            .show()
        {
            eprintln!("could not raise a notification: {e}");
        }

        // The event carries the path, which is all the pipeline needs (§6.3).
        if let Err(e) = handle.emit(CARD_EVENT, &card) {
            eprintln!("could not tell the window about the card: {e}");
        }
    }
}

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
        // Both plugins have to be registered here, not merely depended on and
        // permitted in capabilities/default.json. `NotificationExt` resolves
        // through `Manager::state`, which panics when the type was never
        // managed — so without this line the first detected card panics the
        // watcher thread instead of raising F10's notification, and the
        // `if let Err(..)` below never sees anything to report.
        .plugin(tauri_plugin_notification::init())
        // Launch at login (build plan Phase 14). A LaunchAgent is the macOS
        // mechanism that survives a reboot without asking for privileges.
        // Registering the plugin only makes the setting available; it is off
        // until something turns it on, because a login item somebody did not
        // ask for is a bad surprise.
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
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

            // F10: watch for cards. Held in state so the watch lives as long as
            // the application; dropping the watcher stops it.
            //
            // A machine with no mount root is not an error — it is Linux, or a
            // Mac with `/Volumes` unreadable — and the rest of the application
            // works without card detection, so the failure is reported and
            // stepped over rather than fatal.
            let watcher = detection::VolumeWatcher::start(
                detection::MOUNT_ROOT,
                state.jobs.ledger(),
                notify_card(app.handle().clone()),
            );
            match watcher {
                Ok(watcher) => state.set_card_watcher(watcher),
                Err(e) => eprintln!("card detection is not running: {e}"),
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
            commands::list_roots,
            commands::scan_dates,
            commands::fix_dates,
            commands::plan_rename,
            commands::apply_rename,
            commands::split,
            commands::border,
            commands::tiff_to_jpeg,
            commands::contact_sheet,
            commands::transform,
            commands::summarise_card,
            commands::scan_card,
            commands::stage_card,
            commands::read_card,
            commands::validate_card,
            commands::remediate,
            commands::derive_raw,
            commands::hand_off_card,
            commands::get_launch_at_login,
            commands::set_launch_at_login,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the PhotoTools desktop application");
}
