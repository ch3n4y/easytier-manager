use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use service_manager::{
    LaunchdServiceManager, RestartPolicy, ServiceInstallCtx, ServiceManager, ServiceStartCtx,
    ServiceUninstallCtx,
};

use crate::error::{Error, Result};
use crate::paths::{
    core_path, service_plist_path, service_script_path, state_dir, INSTALL_ROOT, SERVICE_LABEL,
};
use crate::service::ServiceAction;
use crate::util::file_exists;

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
    // Decide from the plist on disk rather than `service-manager`'s status: that
    // call passes a bare label to `launchctl print`, which is rejected for system
    // daemons and resolved to the wrong job (see `service_status`).
    if !Path::new(&service_plist_path()).exists() {
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

/// Query the managed service directly. Returns `(installed, running, pid)`.
///
/// `service-manager`'s status call hands a bare label to `launchctl print`,
/// which rejects it for a system daemon and prints a "did you mean" list. The
/// crate then latches onto the first matching line — which is our own, always
/// running helper daemon — so the managed service permanently reported itself
/// as running. Query the fully-qualified system target instead.
pub fn service_status() -> (bool, bool, i32) {
    if !Path::new(&service_plist_path()).exists() {
        return (false, false, 0);
    }

    if launchd_running() {
        (true, true, running_pid())
    } else {
        (true, false, 0)
    }
}

fn launchd_running() -> bool {
    let target = format!("system/{SERVICE_LABEL}");
    let Ok(output) = Command::new("launchctl").args(["print", &target]).output() else {
        return false;
    };
    if !output.status.success() {
        return false;
    }

    parse_launchd_running(&String::from_utf8_lossy(&output.stdout))
}

/// `launchctl print` reports the job state on a `state = <value>` line.
fn parse_launchd_running(output: &str) -> bool {
    output
        .lines()
        .filter_map(|line| line.trim().strip_prefix("state ="))
        .any(|state| state.trim() == "running")
}

fn running_pid() -> i32 {
    fs::read_to_string(crate::paths::pid_path())
        .ok()
        .and_then(|value| value.trim().parse::<i32>().ok())
        .filter(|pid| *pid > 0)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_launchd_job_state() {
        assert!(parse_launchd_running("\tstate = running\n\tpid = 42\n"));
        assert!(!parse_launchd_running("\tstate = exited\n"));
        assert!(!parse_launchd_running("\tstate = not running\n"));
        assert!(!parse_launchd_running(
            "Bad request.\nCould not find service\n"
        ));
    }
}
