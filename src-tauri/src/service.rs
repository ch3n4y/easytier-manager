//! Service lifecycle for the managed EasyTier core, dispatched to the
//! platform's native service manager: launchd on macOS, the Service Control
//! Manager on Windows.
//!
//! Android has no equivalent to hand the core to — it runs inside the app (see
//! [`crate::mobile_core`]) — so there this module only reports what the linked
//! core is doing.

#[cfg(not(target_os = "android"))]
use std::process::Command;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::error::Result;
#[cfg(not(target_os = "android"))]
use crate::paths::core_path;
#[cfg(not(target_os = "android"))]
use crate::util::file_exists;

#[cfg(target_os = "android")]
use crate::mobile_core;
#[cfg(target_os = "macos")]
use crate::launchd as native;
#[cfg(windows)]
use crate::winservice as native;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceAction {
    Install,
    Start,
    Stop,
    Restart,
}

impl ServiceAction {
    // Only the macOS one-shot authorization path re-invokes the binary with
    // this argument; on Windows the app is already elevated, and on Android
    // there is no second process to re-invoke.
    #[cfg_attr(any(windows, target_os = "android"), allow(dead_code))]
    pub fn as_arg(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
        }
    }

    pub fn from_arg(value: &str) -> Option<Self> {
        match value {
            "install" => Some(Self::Install),
            "start" => Some(Self::Start),
            "stop" => Some(Self::Stop),
            "restart" => Some(Self::Restart),
            _ => None,
        }
    }
}

/// Apply a service lifecycle operation through the platform service manager.
///
/// Desktop only: Android's lifecycle is async and lives in
/// [`crate::platform::apply_service`], because starting the linked core has to
/// await the config server.
#[cfg(desktop)]
pub fn apply_service_action(action: ServiceAction) -> Result<()> {
    native::apply_service_action(action)
}

/// Query the managed service, returning `(installed, running, pid)`.
///
/// Android reports the linked core instead: "installed" is always true (it is
/// part of the APK), "running" means the TUN device is attached and carrying
/// traffic, and there is no pid to report.
pub fn service_status() -> (bool, bool, i32) {
    #[cfg(target_os = "android")]
    {
        (
            mobile_core::is_started(),
            mobile_core::is_running(),
            0,
        )
    }
    #[cfg(not(target_os = "android"))]
    {
        native::service_status()
    }
}

/// Ask the installed core binary for its version, e.g. `2.6.4`.
pub fn installed_version() -> String {
    #[cfg(target_os = "android")]
    {
        // Linked in, so this is a constant rather than something to spawn.
        mobile_core::version().to_string()
    }
    #[cfg(not(target_os = "android"))]
    {
        if !file_exists(&core_path()) {
            return String::new();
        }

        let Ok(output) = version_command().output() else {
            return String::new();
        };
        if !output.status.success() {
            return String::new();
        }

        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        first_version(&text)
    }
}

/// `easytier-core --version`, without a console window.
///
/// The core is a console binary and this runs on every status poll, so spawning
/// it normally would flash a black window up every few seconds.
#[cfg(windows)]
fn version_command() -> Command {
    use std::os::windows::process::CommandExt;

    /// `CREATE_NO_WINDOW` — run the child without a console.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let mut command = Command::new(core_path());
    command.arg("--version");
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

#[cfg(target_os = "macos")]
fn version_command() -> Command {
    let mut command = Command::new(core_path());
    command.arg("--version");
    command
}

/// Pull the first dotted version token out of `easytier-core --version` output.
pub fn first_version(text: &str) -> String {
    Regex::new(r"[0-9]+(?:\.[0-9]+)+")
        .ok()
        .and_then(|pattern| pattern.find(text))
        .map(|found| found.as_str().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_first_version_like_token() {
        assert_eq!(first_version("easytier-core 2.6.4\n"), "2.6.4");
        assert_eq!(first_version("v2.6.4-abc"), "2.6.4");
        assert_eq!(first_version("no version here"), "");
    }

    #[test]
    fn service_action_round_trips_cli_value() {
        for action in [
            ServiceAction::Install,
            ServiceAction::Start,
            ServiceAction::Stop,
            ServiceAction::Restart,
        ] {
            assert!(ServiceAction::from_arg(action.as_arg()).is_some());
        }
        assert!(ServiceAction::from_arg("unknown").is_none());
    }
}
