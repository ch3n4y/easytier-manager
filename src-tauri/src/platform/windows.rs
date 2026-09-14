//! Windows privileged operations. The app runs elevated, so every action here
//! executes directly in-process — no helper, no secondary authorization.

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::app::{run_blocking, set_admin, AppState};
use crate::error::{Error, Result};
use crate::paths::{
    bin_dir, config_dir, default_conf_path, log_path, log_root, service_env_path, state_dir,
    OPTIONAL_LIBRARIES, REQUIRED_BINARIES, REQUIRED_LIBRARIES,
};
use crate::service::ServiceAction;
use crate::util::{copy_file, tail_file};

/// A core that has just been asked to stop can keep its image locked for a
/// moment longer, so an in-place replacement is retried rather than failing with
/// a sharing violation.
const REPLACE_TIMEOUT: Duration = Duration::from_secs(10);
const REPLACE_RETRY_INTERVAL: Duration = Duration::from_millis(250);
/// `ERROR_SHARING_VIOLATION`.
const SHARING_VIOLATION: i32 = 32;

fn elevation_error(elevated: bool) -> String {
    if elevated {
        String::new()
    } else {
        String::from("需要以管理员身份运行")
    }
}

fn current_elevation() -> bool {
    crate::winservice::is_elevated()
}

pub async fn startup(state: &AppState) {
    let elevated = run_blocking(|| Ok(current_elevation())).await.unwrap_or(false);
    set_admin(state, elevated, elevation_error(elevated));
}

pub async fn refresh_admin(state: &AppState) {
    let elevated = run_blocking(|| Ok(current_elevation())).await.unwrap_or(false);
    set_admin(state, elevated, elevation_error(elevated));
}

/// Nothing to set up: the manifest already guarantees an elevated process.
pub async fn prepare(_state: &AppState) -> Result<()> {
    Ok(())
}

pub async fn install_privileged(_state: &AppState, stage: &Path) -> Result<()> {
    let stage = stage.to_path_buf();
    run_blocking(move || apply_staged_install(&stage)).await
}

pub async fn update_privileged(_state: &AppState, stage: &Path) -> Result<()> {
    let stage = stage.to_path_buf();
    run_blocking(move || apply_staged_update(&stage)).await
}

pub async fn apply_service(_state: &AppState, action: ServiceAction) -> Result<()> {
    run_blocking(move || crate::service::apply_service_action(action)).await
}

/// The console address lives in `service.env`, which the service host reads when
/// it starts the core — so a restart is enough to apply a change.
pub async fn write_config_server(_state: &AppState, server: &str) -> Result<()> {
    let server = server.to_string();
    run_blocking(move || write_service_env(&server)).await
}

pub async fn clear_log(_state: &AppState) -> Result<()> {
    run_blocking(clear_log_file).await
}

pub fn read_logs(limit: usize) -> Result<String> {
    tail_file(&log_path(), limit)
}

/// Copy a staged install into the managed directory: binaries and runtime
/// libraries always, configuration only when it does not exist yet.
fn apply_staged_install(stage: &Path) -> Result<()> {
    for dir in [bin_dir(), config_dir(), state_dir(), log_root()] {
        fs::create_dir_all(dir)?;
    }

    copy_staged_binaries(stage)?;

    if !default_conf_path().exists() {
        copy_file(&stage.join("default.conf"), &default_conf_path(), 0o644)?;
    }
    if !service_env_path().exists() {
        copy_file(&stage.join("service.env"), &service_env_path(), 0o644)?;
    }
    Ok(())
}

/// Replace just the binaries, leaving user configuration untouched.
fn apply_staged_update(stage: &Path) -> Result<()> {
    copy_staged_binaries(stage)
}

/// Move the staged core payload into place. The optional WinDivert helpers are
/// copied only when the release staged them.
fn copy_staged_binaries(stage: &Path) -> Result<()> {
    let staged_bin = stage.join("bin");

    for name in REQUIRED_BINARIES.iter().chain(REQUIRED_LIBRARIES.iter()) {
        let name = *name;
        replace_file(&staged_bin.join(name), &bin_dir().join(name))?;
    }

    for name in OPTIONAL_LIBRARIES {
        let src = staged_bin.join(name);
        if src.exists() {
            replace_file(&src, &bin_dir().join(name))?;
        }
    }

    Ok(())
}

/// Write `src` over `dst`, retrying while the outgoing core finishes shutting
/// down and still holds its image open. Only a sharing violation is retried —
/// anything else fails immediately.
fn replace_file(src: &Path, dst: &Path) -> Result<()> {
    let data = fs::read(src)?;
    let deadline = Instant::now() + REPLACE_TIMEOUT;

    loop {
        match fs::write(dst, &data) {
            Ok(()) => return Ok(()),
            Err(err) => {
                let still_running = err.raw_os_error() == Some(SHARING_VIOLATION);
                if !still_running || Instant::now() >= deadline {
                    return Err(Error::msg(format!("{}: {err}", dst.display())));
                }
                std::thread::sleep(REPLACE_RETRY_INTERVAL);
            }
        }
    }
}

fn write_service_env(server: &str) -> Result<()> {
    fs::create_dir_all(config_dir())?;
    fs::write(
        service_env_path(),
        format!("EASYTIER_CONFIG_SERVER={server}\n"),
    )?;
    Ok(())
}

/// Truncate the log in place. Deleting it would not help: the service host holds
/// an open handle and would keep writing to the unlinked file.
fn clear_log_file() -> Result<()> {
    let path = log_path();
    if !path.exists() {
        return Ok(());
    }
    fs::OpenOptions::new().write(true).truncate(true).open(&path)?;
    Ok(())
}
