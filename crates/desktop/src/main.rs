// Prevents additional console window on Windows in release
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use phototools_core::config::Config;
use std::sync::Mutex;
use tauri::State;

struct AppState {
    config: Mutex<Config>,
}

#[tauri::command]
fn get_config(state: State<'_, AppState>) -> Result<Config, String> {
    let config = state.config.lock().unwrap();
    Ok(config.clone())
}

#[tauri::command]
fn save_config(new_config: Config, state: State<'_, AppState>) -> Result<(), String> {
    let mut config = state.config.lock().unwrap();
    new_config.save().map_err(|e| e.to_string())?;
    *config = new_config;
    Ok(())
}

fn main() {
    let config = Config::load().unwrap_or_else(|_| Config::default());

    tauri::Builder::default()
        .manage(AppState {
            config: Mutex::new(config),
        })
        .invoke_handler(tauri::generate_handler![get_config, save_config])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
