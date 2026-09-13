use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

use regex::Regex;
use serde::{Deserialize, Serialize};
use service_manager::{
    LaunchdServiceManager, RestartPolicy, ServiceInstallCtx, ServiceManager, ServiceStartCtx,
    ServiceStatus, ServiceStatusCtx, ServiceUninstallCtx,
};

use crate::error::{Error, Result};
use crate::paths::{
    core_path, pid_path, service_script_path, state_dir, INSTALL_ROOT, SERVICE_LABEL,
};
use crate::util::file_exists;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceAction {
    Install,
    Start,
    Stop,
    Restart,
}

impl ServiceAction {
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

fn manager() -> LaunchdServiceManager {
    LaunchdServiceManager::system()
}

fn label() -> Result<service_manager::ServiceLabel> {
    SERVICE_LABEL.parse().map_err(Error::from)
}

fn install(manager: &LaunchdServiceManager) -> Result<()> {
    if !file_exists(&core_path()) {
        return Err(Error::msg("EasyTier is not installed"));
    }

    // Refresh the wrapper whenever the plist is installed. This also migrates
    // existing installs to wrapper-owned logging now that service-manager owns
    // plist generation and does not expose StandardOutPath.
    let wrapper = service_script_path();
    fs::write(&wrapper, crate::paths::service_wrapper())?;
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755))?;

    manager.install(ServiceInstallCtx {
        label: label()?,
        program: service_script_path(),
        args: Vec::new(),
        contents: None,
        username: None,
        working_directory: Some(PathBuf::from(INSTALL_ROOT)),
        environment: Some(vec![
            (
                "HOME".to_string(),
                state_dir().to_string_lossy().into_owned(),
            ),
            (
                "XDG_STATE_HOME".to_string(),
                state_dir().to_string_lossy().into_owned(),
            ),
        ]),
        autostart: true,
        restart_policy: RestartPolicy::Always { delay_secs: None },
    })?;
    Ok(())
}

fn start(manager: &LaunchdServiceManager) -> Result<()> {
    if matches!(
        manager.status(ServiceStatusCtx { label: label()? })?,
        ServiceStatus::NotInstalled
    ) {
        install(manager)?;
    }
    manager.start(ServiceStartCtx { label: label()? })?;
    Ok(())
}

/// Apply a service lifecycle operation. This function must run as root because
/// the system-level manager writes `/Library/LaunchDaemons`.
pub fn apply_service_action(action: ServiceAction) -> Result<()> {
    let manager = manager();
    match action {
        ServiceAction::Install => {
            install(&manager)?;
            manager.start(ServiceStartCtx { label: label()? })?;
            Ok(())
        }
        ServiceAction::Start => start(&manager),
        ServiceAction::Stop => {
            // launchd immediately revives a KeepAlive service after `stop`, so
            // a user-visible stop is represented by uninstalling the plist.
            manager.uninstall(ServiceUninstallCtx { label: label()? })?;
            Ok(())
        }
        ServiceAction::Restart => {
            manager.uninstall(ServiceUninstallCtx { label: label()? })?;
            install(&manager)?;
            manager.start(ServiceStartCtx { label: label()? })?;
            Ok(())
        }
    }
}

/// Query the managed service through service-manager. Returns
/// `(installed, running, pid)`.
pub fn launchd_status() -> (bool, bool, i32) {
    let Ok(label) = label() else {
        return (false, false, 0);
    };
    let Ok(status) = manager().status(ServiceStatusCtx { label }) else {
        return (false, false, 0);
    };

    match status {
        ServiceStatus::NotInstalled => (false, false, 0),
        ServiceStatus::Stopped(_) => (true, false, 0),
        ServiceStatus::Running => (true, true, running_pid()),
    }
}

fn running_pid() -> i32 {
    fs::read_to_string(pid_path())
        .ok()
        .and_then(|value| value.trim().parse::<i32>().ok())
        .filter(|pid| *pid > 0)
        .unwrap_or(0)
}

/// Ask the installed core binary for its version, e.g. `2.6.4`.
pub fn installed_version() -> String {
    if !file_exists(&core_path()) {
        return String::new();
    }

    let Ok(output) = Command::new(core_path()).arg("--version").output() else {
        return String::new();
    };
    if !output.status.success() {
        return String::new();
    }

    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    first_version(&text)
}

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
