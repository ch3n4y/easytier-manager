//! Shared application entry point.
//!
//! The desktop binary re-invokes this same sheet of code from `main.rs`; Android
//! loads the `cdylib` into the JVM and reaches it through Tauri's mobile entry
//! point. Everything that cannot exist on Android — the tray, the updater, the
//! window sizing, the service-manager plumbing and the downloaded core binary —
//! is compiled out by target rather than stubbed at runtime, so a missing piece
//! is a build error instead of a surprise on the device.

mod app;
mod config;
mod error;
mod paths;
mod platform;
mod service;
mod settings;
mod util;
mod window;

#[cfg(target_os = "macos")]
pub mod helper;
#[cfg(target_os = "macos")]
mod launchd;
#[cfg(windows)]
pub mod winservice;

// Desktop-only: talks to the manager's own updater, which has no Android
// implementation and no APK install channel.
#[cfg(desktop)]
mod app_update;
// Not gated: `release` also owns the `UpdateInfo` DTO and version normalising,
// which the shared commands use on every platform. Only the download half is
// desktop-only, and Android never reaches it.
mod release;
#[cfg(desktop)]
mod tray;

#[cfg(target_os = "android")]
mod apk_installer;
#[cfg(target_os = "android")]
mod app_update_android;
#[cfg(target_os = "android")]
mod mobile_core;

use std::time::Duration;

use app::AppState;
use tauri::Manager;

/// How long the window waits for the frontend to paint before showing itself
/// anyway. Long enough that a normal load never reaches it, short enough that a
/// webview which never loads does not leave the app invisible.
const STARTUP_REVEAL_TIMEOUT: Duration = Duration::from_secs(5);

/// Run one service lifecycle action from `--service-action`, then exit.
///
/// macOS drives this through its one-shot authorization path; on Windows the app
/// is already elevated and calls the service manager in-process instead.
#[cfg(desktop)]
pub fn run_service_action_from_args() {
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // `reqwest` is built with `rustls-no-provider`, so the process has to pick a
    // crypto provider before the first client is constructed — otherwise every
    // HTTPS call panics. Android surfaced this on launch (the core fetches its
    // config over TLS); desktop builds reach the same code, so install it here
    // for both. Installing twice is an error by contract, hence the ignore.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let builder = tauri::Builder::default()
        .manage(AppState::default())
        .setup(setup);

    // The command list is per target on purpose: `generate_handler!` refuses a
    // name that was compiled out, so each platform names exactly what it has.
    #[cfg(desktop)]
    let builder = builder
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            app::get_status,
            app::install_latest,
            app::start_service,
            app::stop_service,
            app::restart_service,
            app::read_config,
            app::save_config,
            app::check_core_update,
            app::list_core_releases,
            app::install_core_version,
            app::update_core,
            app::check_app_update,
            app::install_app_update,
            app::read_logs,
            app::clear_logs,
            app::get_settings,
            app::save_settings,
            app::quit_app,
            window::show_main_window,
        ]);

    // Android owns the tunnel through VpnService, so the shell asks for a TUN
    // plan, hands it to the plugin, and gives the resulting fd back here.
    #[cfg(target_os = "android")]
    let builder = builder
        .plugin(tauri_plugin_vpnservice::init())
        // Needed before `install_app_update` can reach the package installer.
        .plugin(crate::apk_installer::init())
        .invoke_handler(tauri::generate_handler![
            app::get_status,
            app::install_latest,
            app::start_service,
            app::stop_service,
            app::restart_service,
            app::read_config,
            app::save_config,
            app::check_core_update,
            app::list_core_releases,
            app::install_core_version,
            app::update_core,
            app::check_app_update,
            app::install_app_update,
            app::read_logs,
            app::clear_logs,
            app::get_settings,
            app::save_settings,
            app::quit_app,
            app::android_tun_config,
            app::android_attach_tun_fd,
            window::show_main_window,
        ]);

    builder
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn setup(app: &mut tauri::App) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();

    // Pull user settings into memory before anything can need them.
    settings::load(&handle);

    #[cfg(target_os = "android")]
    {
        // Android keeps the managed tree inside its own sandbox, and the app is
        // the only thing that knows where that is. The paths module is told once
        // here so every other module keeps working off the same accessors.
        let data = handle.path().app_data_dir()?;
        let logs = handle.path().app_log_dir()?;
        paths::set_app_dirs(data, logs);

        if let Err(err) = platform::initialize() {
            eprintln!("config layout failed: {err}");
        }
        platform::init_logging();
    }

    #[cfg(desktop)]
    setup_desktop(app)?;

    tauri::async_runtime::spawn(async move {
        platform::startup(handle.state::<AppState>().inner()).await;
    });

    Ok(())
}

/// Tray, window sizing and the hide-on-close trick — the desktop shell's shape.
#[cfg(desktop)]
fn setup_desktop(app: &mut tauri::App) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();
    tray::setup(&handle)?;

    // Closing the window hides it rather than quitting: the app has to stay
    // alive for the tray icon to tell the user anything. Quitting is on the tray
    // menu.
    if let Some(window) = app.get_webview_window("main") {
        // Sized before it is ever shown, so the first frame the user sees is the
        // final one — no empty webview, no visible reshape.
        if let Err(err) = window::apply_startup_size(&window) {
            eprintln!("window sizing failed: {err}");
        }

        // A hidden window is the whole point of the above, so there has to be a
        // way out if the frontend never reports in.
        let unrevealed = window.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(STARTUP_REVEAL_TIMEOUT).await;
            if !unrevealed.is_visible().unwrap_or(true) {
                let _ = unrevealed.show();
            }
        });

        let window_to_hide = window.clone();
        window.on_window_event(move |event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window_to_hide.hide();
            }
        });
    }

    Ok(())
}
