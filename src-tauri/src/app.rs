use std::fs;
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::State;
use tempfile::TempDir;

use crate::config::{read_config_files, ConfigPayload};
use crate::error::{Error, Result};
use crate::helper::{
    ensure_helper, helper_is_ready, is_authorization_cancel, run_admin_script, run_admin_service,
    run_helper_script, run_helper_service,
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
    /// Set once the persistent helper proves uninstallable, so later operations
    /// skip the doomed install prompt and authorize exactly once instead.
    pub one_shot_only: bool,
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
    if ready {
        admin.one_shot_only = false;
    }
}

fn one_shot_only(state: &AppState) -> bool {
    state
        .admin
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .one_shot_only
}

fn set_one_shot_only(state: &AppState, value: bool) {
    let mut admin = state
        .admin
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    admin.one_shot_only = value;
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

/// A privileged operation that can run through the persistent helper or through
/// a one-shot AppleScript authorization.
#[derive(Clone)]
enum PrivilegedOp {
    Script(String),
    Service(ServiceAction),
}

impl PrivilegedOp {
    fn via_helper(&self) -> Result<()> {
        match self {
            Self::Script(script) => run_helper_script(script),
            Self::Service(action) => run_helper_service(*action),
        }
    }

    fn via_one_shot(&self) -> Result<()> {
        match self {
            Self::Script(script) => run_admin_script(script),
            Self::Service(action) => run_admin_service(*action),
        }
    }
}

/// Run a privileged operation with at most one interactive authorization.
///
/// The silent helper socket is always tried first. When the helper is missing
/// we prompt once to install it, and we never chain a second dialog onto the
/// same operation: a declined or failed install is reported instead. Only once
/// the helper is known to be uninstallable do later operations skip the install
/// prompt and authorize exactly once through the one-shot path.
async fn run_privileged_op(state: &AppState, op: PrivilegedOp) -> Result<()> {
    let direct = {
        let op = op.clone();
        run_blocking(move || op.via_helper()).await
    };
    match direct {
        Ok(()) => {
            set_admin(state, true, String::new());
            return Ok(());
        }
        // The helper answered, so this is an operation error rather than an
        // authorization one; prompting again would not change the outcome.
        Err(err) if run_blocking(|| Ok(helper_is_ready())).await? => {
            set_admin(state, true, String::new());
            return Err(err);
        }
        Err(_) => {}
    }

    // An earlier operation already proved the helper cannot be installed.
    if one_shot_only(state) {
        return run_one_shot(state, op).await;
    }

    // A single authorization to install or refresh the persistent helper.
    match run_blocking(ensure_helper).await {
        Ok(()) => {
            let retry = {
                let op = op.clone();
                run_blocking(move || op.via_helper()).await
            };
            set_admin(state, true, String::new());
            retry
        }
        Err(err) => {
            // The install dialog was dismissed or the daemon could not start.
            // Report it instead of opening a second dialog for the same click.
            set_admin(state, false, err.to_string());
            if !is_authorization_cancel(&err) {
                set_one_shot_only(state, true);
            }
            Err(err)
        }
    }
}

async fn run_one_shot(state: &AppState, op: PrivilegedOp) -> Result<()> {
    let fallback = run_blocking(move || op.via_one_shot()).await;
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

/// Authorize before the slow network work in install/update, so the password
/// dialog appears as soon as the user clicks instead of after a download.
async fn warm_helper(state: &AppState) -> Result<()> {
    if one_shot_only(state) {
        return Ok(());
    }
    if run_blocking(|| Ok(helper_is_ready())).await? {
        set_admin(state, true, String::new());
        return Ok(());
    }

    match run_blocking(ensure_helper).await {
        Ok(()) => {
            set_admin(state, true, String::new());
            Ok(())
        }
        Err(err) => {
            set_admin(state, false, err.to_string());
            if !is_authorization_cancel(&err) {
                set_one_shot_only(state, true);
            }
            Err(err)
        }
    }
}

/// Send a script to the privileged helper, installing the daemon on demand and
/// falling back to a one-shot authorization dialog.
async fn run_privileged(state: &AppState, script: &str) -> Result<()> {
    run_privileged_op(state, PrivilegedOp::Script(script.to_string())).await
}

/// Run a typed service-manager operation through the root helper, refreshing
/// an older helper protocol on demand and retaining the one-shot auth fallback.
async fn run_privileged_service(state: &AppState, action: ServiceAction) -> Result<()> {
    run_privileged_op(state, PrivilegedOp::Service(action)).await
}

#[tauri::command]
pub async fn get_status(state: State<'_, AppState>) -> Result<Status> {
    current_status(state.inner()).await
}

#[tauri::command]
pub async fn install_latest(state: State<'_, AppState>) -> Result<Status> {
    warm_helper(state.inner()).await?;

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
    warm_helper(state.inner()).await?;

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
