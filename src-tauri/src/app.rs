use std::fs;
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::State;
use tempfile::TempDir;

use crate::config::{read_config_files, ConfigPayload};
use crate::error::{Error, Result};
use crate::helper::{
    ensure_helper, helper_is_ready, run_admin_script, run_admin_service, run_helper_script,
    run_helper_service,
};
use crate::launchd::{installed_version, launchd_status, ServiceAction};
use crate::paths::{
    config_dir, core_path, install_script, log_path, service_env_path, update_script, LOG_ROOT,
};
use crate::release::{
    download_and_extract, latest_asset, normalize_version, stage_binaries, stage_install_files,
    UpdateInfo,
};
use crate::util::{file_exists, shell_quote, shell_quote_path, tail_file};

#[derive(Default)]
pub struct AdminState {
    pub ready: bool,
    pub error: String,
}

#[derive(Default)]
pub struct AppState {
    pub admin: Mutex<AdminState>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub installed: bool,
    pub running: bool,
    pub loaded: bool,
    pub version: String,
    pub mode: String,
    pub pid: i32,
    pub log_tail: String,
    pub admin_ready: bool,
    pub admin_error: String,
}

/// Run blocking filesystem / process work off the async runtime.
async fn run_blocking<T, F>(task: F) -> Result<T>
where
    F: FnOnce() -> Result<T> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(task).await?
}

fn admin_snapshot(state: &AppState) -> (bool, String) {
    let admin = state
        .admin
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    (admin.ready, admin.error.clone())
}

fn set_admin(state: &AppState, ready: bool, error: String) {
    let mut admin = state
        .admin
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    admin.ready = ready;
    admin.error = error;
}

fn build_status(admin_ready: bool, admin_error: String) -> Status {
    let installed = file_exists(&core_path());
    let (loaded, running, pid) = launchd_status();

    Status {
        installed,
        running,
        loaded,
        version: if installed {
            installed_version()
        } else {
            String::new()
        },
        mode: read_config_files().mode,
        pid,
        log_tail: tail_file(&log_path(), 120).unwrap_or_default(),
        admin_ready,
        admin_error,
    }
}

async fn current_status(state: &AppState) -> Result<Status> {
    // `admin` also carries the last fallback error, but readiness itself is
    // live state. Re-probe on every status poll so a helper that finished
    // starting after a fallback automatically clears the stale "待授权" UI.
    if run_blocking(|| Ok(helper_is_ready())).await? {
        set_admin(state, true, String::new());
    } else {
        let mut admin = state
            .admin
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        admin.ready = false;
    }
    let (ready, error) = admin_snapshot(state);
    run_blocking(move || Ok(build_status(ready, error))).await
}

/// Send a script to the privileged helper, installing the daemon on demand and
/// falling back to a one-shot authorization dialog.
async fn run_privileged(state: &AppState, script: &str) -> Result<()> {
    let owned = script.to_string();

    let direct = {
        let owned = owned.clone();
        run_blocking(move || run_helper_script(&owned)).await
    };
    match direct {
        Ok(()) => {
            set_admin(state, true, String::new());
            return Ok(());
        }
        Err(err) if run_blocking(|| Ok(helper_is_ready())).await? => {
            set_admin(state, true, String::new());
            return Err(err);
        }
        Err(_) => {}
    }

    if run_blocking(ensure_helper).await.is_ok() {
        let retry = {
            let owned = owned.clone();
            run_blocking(move || run_helper_script(&owned)).await
        };
        if retry.is_ok() {
            set_admin(state, true, String::new());
            return Ok(());
        }
    }

    let fallback = {
        let owned = owned.clone();
        run_blocking(move || run_admin_script(&owned)).await
    };
    match &fallback {
        Ok(()) => set_admin(
            state,
            false,
            String::from("helper unavailable; used one-time authorization"),
        ),
        Err(err) => set_admin(state, false, err.to_string()),
    }
    fallback
}

/// Run a typed service-manager operation through the root helper, refreshing
/// an older helper protocol on demand and retaining the one-shot auth fallback.
async fn run_privileged_service(state: &AppState, action: ServiceAction) -> Result<()> {
    let direct = run_blocking(move || run_helper_service(action)).await;
    match direct {
        Ok(()) => {
            set_admin(state, true, String::new());
            return Ok(());
        }
        Err(err) if run_blocking(|| Ok(helper_is_ready())).await? => {
            set_admin(state, true, String::new());
            return Err(err);
        }
        Err(_) => {}
    }

    if run_blocking(ensure_helper).await.is_ok() {
        let retry = run_blocking(move || run_helper_service(action)).await;
        if retry.is_ok() {
            set_admin(state, true, String::new());
            return Ok(());
        }
    }

    let fallback = run_blocking(move || run_admin_service(action)).await;
    match &fallback {
        Ok(()) => set_admin(
            state,
            false,
            String::from("helper unavailable; used one-time authorization"),
        ),
        Err(err) => set_admin(state, false, err.to_string()),
    }
    fallback
}

#[tauri::command]
pub async fn get_status(state: State<'_, AppState>) -> Result<Status> {
    current_status(state.inner()).await
}

#[tauri::command]
pub async fn install_latest(state: State<'_, AppState>) -> Result<Status> {
    let (release, asset) = latest_asset().await?;

    let stage = TempDir::new()?;
    download_and_extract(&asset.browser_download_url, stage.path()).await?;

    let stage_path = stage.path().to_path_buf();
    let tag = release.tag_name.clone();
    run_blocking(move || stage_install_files(&stage_path, &tag)).await?;

    let script = install_script(stage.path());
    run_privileged(state.inner(), &script).await?;
    run_privileged_service(state.inner(), ServiceAction::Install).await?;
    tokio::time::sleep(Duration::from_millis(400)).await;
    current_status(state.inner()).await
}

#[tauri::command]
pub async fn start_service(state: State<'_, AppState>) -> Result<Status> {
    run_privileged_service(state.inner(), ServiceAction::Start).await?;
    tokio::time::sleep(Duration::from_millis(400)).await;
    current_status(state.inner()).await
}

#[tauri::command]
pub async fn stop_service(state: State<'_, AppState>) -> Result<Status> {
    run_privileged_service(state.inner(), ServiceAction::Stop).await?;
    tokio::time::sleep(Duration::from_millis(400)).await;
    current_status(state.inner()).await
}

#[tauri::command]
pub async fn restart_service(state: State<'_, AppState>) -> Result<Status> {
    run_privileged_service(state.inner(), ServiceAction::Restart).await?;
    tokio::time::sleep(Duration::from_millis(600)).await;
    current_status(state.inner()).await
}

#[tauri::command]
pub async fn read_config() -> Result<ConfigPayload> {
    run_blocking(|| Ok(read_config_files())).await
}

#[tauri::command]
pub async fn save_config(
    state: State<'_, AppState>,
    payload: ConfigPayload,
) -> Result<ConfigPayload> {
    let server = payload.config_server.trim().to_string();
    if server.is_empty() {
        return Err(Error::msg("config server is required"));
    }

    let stage = TempDir::new()?;
    let staged_env = stage.path().join("service.env");
    fs::write(
        &staged_env,
        format!("EASYTIER_CONFIG_SERVER={}\n", shell_quote(&server)),
    )?;

    let script = [
        String::from("set -eu"),
        format!("mkdir -p {}", shell_quote_path(&config_dir())),
        format!(
            "install -m 644 {} {}",
            shell_quote_path(&staged_env),
            shell_quote_path(&service_env_path())
        ),
    ]
    .join("\n");

    run_privileged(state.inner(), &script).await?;
    Ok(read_config_files())
}

#[tauri::command]
pub async fn check_core_update() -> Result<UpdateInfo> {
    let (release, asset) = latest_asset().await?;

    run_blocking(move || {
        let current = installed_version();
        let latest = normalize_version(&release.tag_name);
        Ok(UpdateInfo {
            installed: file_exists(&core_path()),
            has_update: current.is_empty() || current != latest,
            current_version: current,
            latest_version: latest,
            asset_name: asset.name,
        })
    })
    .await
}

#[tauri::command]
pub async fn update_core(state: State<'_, AppState>) -> Result<Status> {
    let before = current_status(state.inner()).await?;

    let (release, asset) = latest_asset().await?;
    if before.version == normalize_version(&release.tag_name) {
        return Ok(before);
    }

    let stage = TempDir::new()?;
    download_and_extract(&asset.browser_download_url, stage.path()).await?;

    let stage_path = stage.path().to_path_buf();
    run_blocking(move || stage_binaries(&stage_path)).await?;

    if before.loaded {
        run_privileged_service(state.inner(), ServiceAction::Stop).await?;
    }

    let script = update_script(stage.path());
    let update_result = run_privileged(state.inner(), &script).await;
    let restart_result = if before.running {
        run_privileged_service(state.inner(), ServiceAction::Start).await
    } else {
        Ok(())
    };

    match (update_result, restart_result) {
        (Err(update), Err(restart)) => {
            return Err(Error::msg(format!(
                "update failed: {update}; restoring service failed: {restart}"
            )))
        }
        (Err(update), _) => return Err(update),
        (_, Err(restart)) => return Err(restart),
        (Ok(()), Ok(())) => {}
    }
    tokio::time::sleep(Duration::from_millis(600)).await;
    current_status(state.inner()).await
}

#[tauri::command]
pub async fn read_logs(limit: i64) -> Result<String> {
    let limit = if limit <= 0 || limit > 1000 {
        200usize
    } else {
        limit as usize
    };
    run_blocking(move || tail_file(&log_path(), limit)).await
}

#[tauri::command]
pub async fn clear_logs(state: State<'_, AppState>) -> Result<Status> {
    let script = [
        String::from("set -eu"),
        format!("mkdir -p {}", shell_quote(LOG_ROOT)),
        format!(": > {}", shell_quote_path(&log_path())),
    ]
    .join("\n");

    run_privileged(state.inner(), &script).await?;
    current_status(state.inner()).await
}
