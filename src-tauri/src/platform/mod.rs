//! Privileged operations, dispatched per platform.
//!
//! macOS funnels every privileged action through a root LaunchDaemon helper
//! (with a one-shot `osascript` authorization fallback) because the GUI runs as
//! the invoking user. Windows launches the whole app elevated, so the same
//! operations execute in-process.
//!
//! Both platform modules expose the same surface:
//!
//! - `startup` / `refresh_admin` — establish privilege state for the UI
//! - `prepare` — make the privilege boundary usable before a slow operation
//! - `install_privileged` / `update_privileged` — copy staged files into place
//! - `apply_service` — install/start/stop/restart the managed service
//! - `write_config_server` / `clear_log` / `read_logs` — config and log access

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "macos")]
pub use macos::*;
#[cfg(windows)]
pub use windows::*;
