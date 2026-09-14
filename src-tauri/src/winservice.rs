//! Windows service management for the managed EasyTier core.
//!
//! The service runs *this* binary in a `--service-host` mode that supervises the
//! core as a child process and captures its stdout/stderr into the managed log
//! file. Registering `easytier-core.exe` directly does not work for logging:
//! EasyTier's own `--file-log-level` / `--file-log-dir` flags create the file but
//! never write to it, and a Windows service has no stdout to fall back on. The
//! wrapper restores the same behaviour macOS gets from its shell script.
//!
//! Unlike macOS — where launchd's `KeepAlive` forces a "stop means uninstall"
//! design — the SCM keeps a stopped service registered, so stop/start map
//! directly onto the service lifecycle.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::mem::{size_of, zeroed};
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use windows_service::service::{
    Service, ServiceAccess, ServiceControl, ServiceControlAccept, ServiceDependency,
    ServiceErrorControl, ServiceExitCode, ServiceInfo, ServiceStartType, ServiceState,
    ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
use windows_service::{define_windows_service, service_dispatcher};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use winreg::enums::HKEY_LOCAL_MACHINE;
use winreg::RegKey;

use crate::error::{Error, Result};
use crate::paths::{
    core_path, default_conf_path, install_root, log_path, log_root, pid_path, state_dir,
    WINDOWS_SERVICE_DISPLAY_NAME, WINDOWS_SERVICE_NAME,
};
use crate::service::ServiceAction;
use crate::util::file_exists;

const SERVICE_TYPE: ServiceType = ServiceType::OWN_PROCESS;
const SERVICE_DESCRIPTION: &str = "EasyTier 核心服务";

/// Argument the SCM launches this binary with to run as the service host.
const SERVICE_HOST_ARG: &str = "--service-host";

/// Registry location EasyTier reads to recover a service's working directory.
const WIN_SERVICE_WORK_DIR_REG_KEY: &str = r"SOFTWARE\EasyTier\Service\WorkDir";

/// Keep a supervised child from flashing up a console window.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// How often the supervisor checks on the child and its stop signal.
const SUPERVISE_INTERVAL: Duration = Duration::from_millis(500);

/// How long to wait for the SCM to settle into a requested state, and how often
/// to poll for it.
const STATE_TIMEOUT: Duration = Duration::from_secs(20);
const STATE_POLL_INTERVAL: Duration = Duration::from_millis(200);

/// True when this process was launched by the SCM to supervise the core.
pub fn is_service_host_invocation() -> bool {
    std::env::args().nth(1).as_deref() == Some(SERVICE_HOST_ARG)
}

fn manager(access: ServiceManagerAccess) -> Result<ServiceManager> {
    Ok(ServiceManager::local_computer(None::<&str>, access)?)
}

fn service_name() -> OsString {
    OsString::from(WINDOWS_SERVICE_NAME)
}

/// True when this process can drive the SCM — i.e. it is elevated. The app
/// manifest requests `requireAdministrator`, so this is normally always true;
/// probing keeps the UI honest if the binary is launched some other way.
pub fn is_elevated() -> bool {
    ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT).is_ok()
}

// ── Service host ──────────────────────────────────────────────────────────

/// Hand control to the SCM and supervise the core. Never returns.
pub fn run_service_host() -> ! {
    if let Err(err) = service_dispatcher::start(WINDOWS_SERVICE_NAME, ffi_service_main) {
        eprintln!("failed to start the service dispatcher: {err}");
        std::process::exit(1);
    }
    std::process::exit(0)
}

define_windows_service!(ffi_service_main, service_main);

fn service_main(_arguments: Vec<OsString>) {
    if let Err(err) = serve() {
        // There is no console here; the log the supervisor wrote is the record.
        eprintln!("service host stopped with an error: {err}");
        std::process::exit(1);
    }
}

fn serve() -> Result<()> {
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let handler = move |control| match control {
        ServiceControl::Stop | ServiceControl::Shutdown => {
            let _ = stop_tx.send(());
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    };

    let status_handle = service_control_handler::register(WINDOWS_SERVICE_NAME, handler)?;
    status_handle.set_service_status(status_report(
        ServiceState::Running,
        ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
    ))?;

    let outcome = supervise(stop_rx);

    let _ = status_handle.set_service_status(status_report(
        ServiceState::Stopped,
        ServiceControlAccept::empty(),
    ));
    outcome
}

fn status_report(state: ServiceState, controls: ServiceControlAccept) -> ServiceStatus {
    ServiceStatus {
        service_type: SERVICE_TYPE,
        current_state: state,
        controls_accepted: controls,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    }
}

/// Run the core as a child, streaming its output into the managed log, until it
/// exits or the SCM asks us to stop.
///
/// The core owns the Wintun adapter, so it must never outlive this process: an
/// orphan keeps the adapter occupied and every later start then fails with
/// `WintunStartSession failed`.
fn supervise(stop_rx: Receiver<()>) -> Result<()> {
    for dir in [log_root(), state_dir()] {
        fs::create_dir_all(dir)?;
    }

    let stdout = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path())?;
    let stderr = stdout.try_clone()?;

    let job = KillOnCloseJob::new();
    let mut child = SupervisedChild::spawn(stdout, stderr)?;
    if let Some(job) = &job {
        job.assign(child.handle());
    }

    fs::write(pid_path(), child.pid().to_string())?;

    let outcome = loop {
        if stop_rx.try_recv().is_ok() {
            break Ok(());
        }

        match child.try_wait() {
            Ok(Some(status)) => {
                break Err(Error::msg(format!(
                    "easytier-core exited unexpectedly: {status}"
                )))
            }
            Ok(None) => std::thread::sleep(SUPERVISE_INTERVAL),
            Err(err) => break Err(err.into()),
        }
    };

    let _ = fs::remove_file(pid_path());
    outcome
}

/// The supervised core. Dropping it terminates the process, so no exit path —
/// a stop request, an early error return, or a panic — can orphan it.
struct SupervisedChild(Child);

impl SupervisedChild {
    fn spawn(stdout: fs::File, stderr: fs::File) -> Result<Self> {
        let child = Command::new(core_path())
            .args(core_args())
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|err| Error::msg(format!("failed to start easytier-core: {err}")))?;
        Ok(Self(child))
    }

    fn pid(&self) -> u32 {
        self.0.id()
    }

    fn handle(&self) -> HANDLE {
        self.0.as_raw_handle() as HANDLE
    }

    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.0.try_wait()
    }
}

impl Drop for SupervisedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A job object that terminates everything inside it once its handle closes.
///
/// This covers the case the child guard cannot: the host being terminated
/// outright. The kernel closes every handle as the process dies, which closes
/// the job and takes the core with it.
struct KillOnCloseJob(HANDLE);

impl KillOnCloseJob {
    fn new() -> Option<Self> {
        // SAFETY: the resulting handle is owned by this struct and closed once,
        // in `Drop`.
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return None;
            }

            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let applied = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                std::ptr::addr_of!(limits).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if applied == 0 {
                let _ = CloseHandle(job);
                return None;
            }

            Some(Self(job))
        }
    }

    /// Put the freshly spawned core into the job. Best effort: the child guard
    /// still cleans up on any orderly exit.
    fn assign(&self, process: HANDLE) {
        // SAFETY: `process` is a live handle to the child just spawned, and the
        // job handle is owned by `self`.
        unsafe {
            let _ = AssignProcessToJobObject(self.0, process);
        }
    }
}

impl Drop for KillOnCloseJob {
    fn drop(&mut self) {
        // SAFETY: the handle came from `CreateJobObjectW` and is closed once.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// The core's arguments, resolved at start time so changing the console address
/// only needs a restart — the same contract the macOS wrapper has.
fn core_args_for(server: &str) -> Vec<OsString> {
    if server.is_empty() {
        vec![
            OsString::from("-c"),
            default_conf_path().into_os_string(),
        ]
    } else {
        vec![OsString::from("-w"), OsString::from(server)]
    }
}

fn core_args() -> Vec<OsString> {
    core_args_for(&crate::config::read_config_files().config_server)
}

// ── Service lifecycle ─────────────────────────────────────────────────────

fn service_info() -> Result<ServiceInfo> {
    if !file_exists(&core_path()) {
        return Err(Error::msg("EasyTier is not installed"));
    }

    Ok(ServiceInfo {
        name: service_name(),
        display_name: OsString::from(WINDOWS_SERVICE_DISPLAY_NAME),
        service_type: SERVICE_TYPE,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: std::env::current_exe()?,
        launch_arguments: vec![OsString::from(SERVICE_HOST_ARG)],
        dependencies: vec![
            ServiceDependency::Service(OsString::from("rpcss")),
            ServiceDependency::Service(OsString::from("dnscache")),
        ],
        account_name: None, // LocalSystem
        account_password: None,
    })
}

/// Register the service, or correct it in place when it already exists. This
/// also migrates a service an older build registered against `easytier-core.exe`.
fn install(manager: &ServiceManager) -> Result<()> {
    let info = service_info()?;

    let service = match manager.open_service(service_name(), ServiceAccess::CHANGE_CONFIG) {
        Ok(service) => {
            service.change_config(&info)?;
            service
        }
        Err(_) => manager.create_service(&info, ServiceAccess::CHANGE_CONFIG)?,
    };

    service.set_description(SERVICE_DESCRIPTION)?;
    set_work_directory()?;
    Ok(())
}

fn start(manager: &ServiceManager) -> Result<()> {
    // Re-assert the record before starting, so a service registered by an older
    // build (or by hand) is corrected rather than started with stale settings.
    install(manager)?;

    let access = ServiceAccess::START | ServiceAccess::QUERY_STATUS;
    let service = manager.open_service(service_name(), access)?;

    match service.query_status()?.current_state {
        ServiceState::Running => return Ok(()),
        // `stop`/`start` in this crate only *send* the control — they return as
        // soon as the SCM accepts it. A start issued while the previous stop is
        // still settling is rejected, so wait for the stop to land first.
        ServiceState::StopPending => {
            wait_for_state(&service, ServiceState::Stopped)?;
        }
        ServiceState::StartPending => {
            return wait_for_state(&service, ServiceState::Running);
        }
        _ => {}
    }

    service.start(&[] as &[&OsStr])?;
    wait_for_state(&service, ServiceState::Running)
}

fn stop(manager: &ServiceManager) -> Result<()> {
    let service = manager.open_service(
        service_name(),
        ServiceAccess::STOP | ServiceAccess::QUERY_STATUS,
    )?;
    if service.query_status()?.current_state == ServiceState::Stopped {
        return Ok(());
    }

    service.stop()?;
    // Waiting matters beyond tidiness: the SCM only reports `Stopped` once the
    // service host has torn its core down, and that is what releases the lock on
    // the core binary an in-place update needs to replace.
    wait_for_state(&service, ServiceState::Stopped)
}

/// Poll until the service reaches `target`. The SCM transitions through pending
/// states, so a control that was accepted is not yet a control that took effect.
fn wait_for_state(service: &Service, target: ServiceState) -> Result<()> {
    let deadline = Instant::now() + STATE_TIMEOUT;
    loop {
        let state = service.query_status()?.current_state;
        if state == target {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(Error::msg(format!(
                "service did not reach {target:?} in time (still {state:?})"
            )));
        }
        std::thread::sleep(STATE_POLL_INTERVAL);
    }
}

/// Apply a service lifecycle operation. This function must run as administrator
/// because the SCM rejects service creation and control otherwise.
pub fn apply_service_action(action: ServiceAction) -> Result<()> {
    let manager = manager(ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE)?;
    match action {
        ServiceAction::Install => {
            install(&manager)?;
            start(&manager)
        }
        ServiceAction::Start => start(&manager),
        ServiceAction::Stop => stop(&manager),
        ServiceAction::Restart => {
            stop(&manager)?;
            start(&manager)
        }
    }
}

/// Query the managed service directly. Returns `(installed, running, pid)`.
pub fn service_status() -> (bool, bool, i32) {
    let Ok(manager) = manager(ServiceManagerAccess::CONNECT) else {
        return (false, false, 0);
    };
    let Ok(service) = manager.open_service(service_name(), ServiceAccess::QUERY_STATUS) else {
        return (false, false, 0);
    };

    match service.query_status() {
        Ok(status) => {
            let running = status.current_state == ServiceState::Running;
            (true, running, if running { running_pid() } else { 0 })
        }
        Err(_) => (false, false, 0),
    }
}

/// The core's pid, recorded by the service host.
fn running_pid() -> i32 {
    fs::read_to_string(pid_path())
        .ok()
        .and_then(|value| value.trim().parse::<i32>().ok())
        .filter(|pid| *pid > 0)
        .unwrap_or(0)
}

/// Record the service working directory the way EasyTier's core expects to
/// find it. Because the app always passes absolute paths this is belt and
/// braces, but it keeps state resolution identical to a native EasyTier install.
fn set_work_directory() -> Result<()> {
    let root = install_root();
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let (key, _) = hklm.create_subkey(WIN_SERVICE_WORK_DIR_REG_KEY)?;
    key.set_value(WINDOWS_SERVICE_NAME, &root.to_string_lossy().to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_config_is_used_without_a_console_address() {
        let args = core_args_for("");
        assert_eq!(args[0], OsString::from("-c"));
        assert_eq!(args[1], default_conf_path().into_os_string());
        // The core's own file logger does not work on Windows, so logging is
        // owned by the service host instead of passed through as flags.
        assert!(!args.contains(&OsString::from("--file-log-dir")));
    }

    #[test]
    fn configured_console_address_replaces_the_config_file() {
        let args = core_args_for("udp://config.example.com:22020/admin");
        assert_eq!(args[0], OsString::from("-w"));
        assert_eq!(
            args[1],
            OsString::from("udp://config.example.com:22020/admin")
        );
    }
}
