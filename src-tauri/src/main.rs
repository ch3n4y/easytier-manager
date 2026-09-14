// Prevents an additional console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod config;
mod error;
#[cfg(target_os = "macos")]
mod helper;
#[cfg(target_os = "macos")]
mod launchd;
mod paths;
mod platform;
mod release;
mod service;
mod settings;
mod tray;
mod util;
mod window;
#[cfg(windows)]
mod winservice;

use app::AppState;
use tauri::Manager;

fn main() {
    // macOS: the installed LaunchDaemon re-invokes this same binary as the
    // privileged helper, so bail out before any windowing code runs.
    #[cfg(target_os = "macos")]
    if helper::is_helper_invocation() {
        helper::run_helper();
    }

    // Windows: the registered service is this same binary in service-host mode.
    // Hand control to the dispatcher before any windowing code runs.
    #[cfg(windows)]
    if winservice::is_service_host_invocation() {
        winservice::run_service_host();
    }

    // Hidden recovery entry point: run one service lifecycle action and exit.
    // macOS drives this through its one-shot authorization path; on Windows the
    // app is already elevated and calls the service manager in-process.
    if std::env::args().nth(1).as_deref() == Some("--service-action") {
        let result = std::env::args()
            .nth(2)
            .as_deref()
            .and_then(service::ServiceAction::from_arg)
            .ok_or_else(|| error::Error::msg("invalid service action"))
            .and_then(service::apply_service_action);
        match result {
            Ok(()) => std::process::exit(0),
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
    }

    tauri::Builder::default()
        .manage(AppState::default())
        .setup(|app| {
            let handle = app.handle().clone();
            // Pull user settings into memory before anything can need them.
            settings::load(&handle);

            tray::setup(&handle)?;

            // Closing the window hides it rather than quitting: the app has to
            // stay alive for the tray icon to tell the user anything. Quitting is
            // on the tray menu.
            if let Some(window) = app.get_webview_window("main") {
                let window_to_hide = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = window_to_hide.hide();
                    }
                });
            }

            tauri::async_runtime::spawn(async move {
                platform::startup(handle.state::<AppState>().inner()).await;
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app::get_status,
            app::install_latest,
            app::start_service,
            app::stop_service,
            app::restart_service,
            app::read_config,
            app::save_config,
            app::check_core_update,
            app::update_core,
            app::read_logs,
            app::clear_logs,
            app::get_settings,
            app::save_settings,
            window::set_window_mode,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
