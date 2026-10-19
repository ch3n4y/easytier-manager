// Prevents an additional console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // macOS: the installed LaunchDaemon re-invokes this same binary as the
    // privileged helper, so bail out before any windowing code runs.
    #[cfg(target_os = "macos")]
    if easytier_manager_lib::helper::is_helper_invocation() {
        easytier_manager_lib::helper::run_helper();
    }

    // Windows: the registered service is this same binary in service-host mode.
    // Hand control to the dispatcher before any windowing code runs.
    #[cfg(windows)]
    if easytier_manager_lib::winservice::is_service_host_invocation() {
        easytier_manager_lib::winservice::run_service_host();
    }

    // Hidden recovery entry point: run one service lifecycle action and exit.
    // macOS drives this through its one-shot authorization path; on Windows the
    // app is already elevated and calls the service manager in-process.
    #[cfg(desktop)]
    if std::env::args().nth(1).as_deref() == Some("--service-action") {
        easytier_manager_lib::run_service_action_from_args();
    }

    easytier_manager_lib::run();
}
