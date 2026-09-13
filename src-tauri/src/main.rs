// Prevents an additional console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod config;
mod error;
mod helper;
mod launchd;
mod paths;
mod release;
mod util;

use app::AppState;
use tauri::Manager;

fn main() {
    // The installed LaunchDaemon re-invokes this same binary as the privileged
    // helper, so bail out before any windowing code runs.
    if helper::is_helper_invocation() {
        helper::run_helper();
    }
    if std::env::args().nth(1).as_deref() == Some("--service-action") {
        let result = std::env::args()
            .nth(2)
            .as_deref()
            .and_then(launchd::ServiceAction::from_arg)
            .ok_or_else(|| error::Error::msg("invalid service action"))
            .and_then(launchd::apply_service_action);
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
            tauri::async_runtime::spawn(async move {
                let outcome = tokio::task::spawn_blocking(helper::ensure_helper).await;
                let state = handle.state::<AppState>();
                let mut admin = state
                    .admin
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());

                match outcome {
                    Ok(Ok(())) => {
                        admin.ready = true;
                        admin.error.clear();
                    }
                    Ok(Err(err)) => {
                        admin.ready = false;
                        admin.error = err.to_string();
                    }
                    Err(err) => {
                        admin.ready = false;
                        admin.error = err.to_string();
                    }
                }
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
