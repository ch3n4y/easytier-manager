use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
#[cfg(desktop)]
use tauri_plugin_updater::Update;

#[cfg(desktop)]
use crate::app_update::{self, AppUpdateInfo};
use crate::config::{read_config_files, ConfigPayload};
use crate::error::{Error, Result};
use crate::paths::core_path;
use crate::platform;
use crate::service::{self, ServiceAction};
use crate::settings::{self, Settings};
use crate::util::file_exists;

#[cfg(desktop)]
use tempfile::TempDir;

#[cfg(desktop)]
use crate::release::{
    download_and_extract, latest_asset, normalize_version, stage_binaries, stage_install_files,
};
use crate::release::UpdateInfo;

#[derive(Default)]
pub struct AdminState {
    pub ready: bool,
    pub error: String,
    /// Set once the persistent helper proves uninstallable, so later operations
    /// skip the doomed install prompt and authorize exactly once instead.
    /// macOS-only: Windows has no fallback authorization path.
    #[cfg_attr(any(windows, target_os = "android"), allow(dead_code))]
    pub one_shot_only: bool,
}

#[derive(Default)]
pub struct AppState {
    pub admin: Mutex<AdminState>,
    /// The manager release `check_app_update` resolved, held until the user
    /// decides to install it. Re-resolving at install time could install a
    /// version other than the one the panel advertised.
    ///
    /// Desktop only: there is no in-app update channel for an APK.
    #[cfg(desktop)]
    pub pending_update: Mutex<Option<Update>>,
}

#[cfg(target_os = "macos")]
const PLATFORM: &str = "macos";
#[cfg(windows)]
const PLATFORM: &str = "windows";
#[cfg(target_os = "android")]
const PLATFORM: &str = "android";

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
    /// Lets the UI branch authorization copy per platform.
    pub platform: String,
    /// The manager's own version, as opposed to `version`, which is the
    /// installed EasyTier core's. Kept in step with the updater, which compares
    /// releases against the same value.
    pub app_version: String,
}

/// Run blocking filesystem / process work off the async runtime.
pub(crate) async fn run_blocking<T, F>(task: F) -> Result<T>
where
    F: FnOnce() -> Result<T> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(task).await?
}

pub(crate) fn admin_snapshot(state: &AppState) -> (bool, String) {
    let admin = state
        .admin
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    (admin.ready, admin.error.clone())
}

pub(crate) fn set_admin(state: &AppState, ready: bool, error: String) {
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

#[cfg(target_os = "macos")]
pub(crate) fn one_shot_only(state: &AppState) -> bool {
    state
        .admin
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .one_shot_only
}

#[cfg(target_os = "macos")]
pub(crate) fn set_one_shot_only(state: &AppState, value: bool) {
    let mut admin = state
        .admin
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    admin.one_shot_only = value;
}

/// Whether the core is available to run.
///
/// Desktop asks the filesystem, because the core is a downloaded binary.
/// Android links it into the APK, so it is always there.
fn core_installed() -> bool {
    #[cfg(target_os = "android")]
    {
        true
    }
    #[cfg(not(target_os = "android"))]
    {
        file_exists(&core_path())
    }
}

fn build_status(admin_ready: bool, admin_error: String, app_version: String) -> Status {
    let installed = core_installed();
    let (loaded, running, pid) = service::service_status();

    Status {
        installed,
        running,
        loaded,
        version: if installed {
            service::installed_version()
        } else {
            String::new()
        },
        mode: read_config_files().mode,
        pid,
        admin_ready,
        admin_error,
        platform: String::from(PLATFORM),
        app_version,
    }
}

async fn current_status(app: &AppHandle, state: &AppState) -> Result<Status> {
    platform::refresh_admin(state).await;
    let (ready, error) = admin_snapshot(state);
    let app_version = app.package_info().version.to_string();
    run_blocking(move || Ok(build_status(ready, error, app_version))).await
}

/// The service manager accepts an action before it has finished applying it, so
/// every lifecycle command lets it settle before reading the status back.
const SERVICE_SETTLE_MS: u64 = 400;
/// A restart takes longer than a start or a stop.
const SERVICE_RESTART_SETTLE_MS: u64 = 600;

/// Wait for the service manager to settle, then read the resulting status.
async fn settle_and_report(app: &AppHandle, state: &AppState, settle_ms: u64) -> Result<Status> {
    tokio::time::sleep(Duration::from_millis(settle_ms)).await;
    current_status(app, state).await
}

/// Apply a lifecycle action, then report the status it produced.
async fn apply_and_report(
    app: &AppHandle,
    state: &AppState,
    action: ServiceAction,
    settle_ms: u64,
) -> Result<Status> {
    platform::apply_service(state, action).await?;
    settle_and_report(app, state, settle_ms).await
}

/// Download progress handed to the webview while an asset is fetched. Shared by
/// the EasyTier core download and the manager's own, so the frontend keeps a
/// single listener.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DownloadProgress {
    pub(crate) received: u64,
    pub(crate) total: Option<u64>,
}

/// A reporter that forwards download progress to the frontend. Release assets
/// are large and often slow, so without this the UI looks wedged.
#[cfg(desktop)]
fn progress_reporter(app: &AppHandle) -> impl Fn(u64, Option<u64>) + 'static {
    let app = app.clone();
    move |received, total| {
        let _ = app.emit("download-progress", DownloadProgress { received, total });
    }
}

#[tauri::command]
pub async fn get_status(app: AppHandle, state: State<'_, AppState>) -> Result<Status> {
    let status = current_status(&app, state.inner()).await?;
    // The tray is the only sign of life once the window is hidden.
    #[cfg(desktop)]
    crate::tray::update_tooltip(&app, &status);
    Ok(status)
}

#[tauri::command]
pub async fn install_latest(app: AppHandle, state: State<'_, AppState>) -> Result<Status> {
    #[cfg(desktop)]
    {
        platform::prepare(state.inner()).await?;

        let (release, asset) = latest_asset().await?;

        let stage = TempDir::new()?;
        download_and_extract(
            &asset.browser_download_url,
            stage.path(),
            progress_reporter(&app),
        )
        .await?;

        let stage_path = stage.path().to_path_buf();
        let tag = release.tag_name.clone();
        run_blocking(move || stage_install_files(&stage_path, &tag)).await?;

        platform::install_privileged(state.inner(), stage.path()).await?;
        apply_and_report(&app, state.inner(), ServiceAction::Install, SERVICE_SETTLE_MS).await
    }
    // The core is part of the APK, so installing means laying out the config
    // files (done at startup) and starting it.
    #[cfg(target_os = "android")]
    {
        platform::prepare(state.inner()).await?;
        apply_and_report(&app, state.inner(), ServiceAction::Install, SERVICE_SETTLE_MS).await
    }
}

#[tauri::command]
pub async fn start_service(app: AppHandle, state: State<'_, AppState>) -> Result<Status> {
    apply_and_report(&app, state.inner(), ServiceAction::Start, SERVICE_SETTLE_MS).await
}

#[tauri::command]
pub async fn stop_service(app: AppHandle, state: State<'_, AppState>) -> Result<Status> {
    apply_and_report(&app, state.inner(), ServiceAction::Stop, SERVICE_SETTLE_MS).await
}

#[tauri::command]
pub async fn restart_service(app: AppHandle, state: State<'_, AppState>) -> Result<Status> {
    apply_and_report(
        &app,
        state.inner(),
        ServiceAction::Restart,
        SERVICE_RESTART_SETTLE_MS,
    )
    .await
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

    platform::write_config_server(state.inner(), &server).await?;
    Ok(read_config_files())
}

#[tauri::command]
pub async fn check_core_update() -> Result<UpdateInfo> {
    #[cfg(desktop)]
    {
        let (release, asset) = latest_asset().await?;

        run_blocking(move || {
            let current = service::installed_version();
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
    // The linked core cannot be updated apart from the app, so there is never a
    // newer core than the one this build shipped with.
    #[cfg(target_os = "android")]
    {
        run_blocking(|| {
            let version = service::installed_version();
            Ok(UpdateInfo {
                installed: true,
                has_update: false,
                current_version: version.clone(),
                latest_version: version,
                asset_name: String::new(),
            })
        })
        .await
    }
}

#[tauri::command]
pub async fn update_core(app: AppHandle, state: State<'_, AppState>) -> Result<Status> {
    #[cfg(desktop)]
    {
        platform::prepare(state.inner()).await?;

        let before = current_status(&app, state.inner()).await?;

        let (release, asset) = latest_asset().await?;
        if before.version == normalize_version(&release.tag_name) {
            return Ok(before);
        }

        let stage = TempDir::new()?;
        download_and_extract(
            &asset.browser_download_url,
            stage.path(),
            progress_reporter(&app),
        )
        .await?;

        let stage_path = stage.path().to_path_buf();
        run_blocking(move || stage_binaries(&stage_path)).await?;

        if before.loaded {
            platform::apply_service(state.inner(), ServiceAction::Stop).await?;
        }

        let update_result = platform::update_privileged(state.inner(), stage.path()).await;
        let restart_result = if before.running {
            platform::apply_service(state.inner(), ServiceAction::Start).await
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
        settle_and_report(&app, state.inner(), SERVICE_RESTART_SETTLE_MS).await
    }
    // Nothing to fetch: the core is linked in, and replacing it means replacing
    // the app. Say so instead of pretending to succeed.
    #[cfg(target_os = "android")]
    {
        let _ = (&app, &state);
        Err(Error::msg(
            "EasyTier 核心随应用一起升级，请通过应用商店或重新安装 APK 更新",
        ))
    }
}

/// Resolve the manager's own release channel and remember what it offered, so
/// the install command can use exactly this release.
#[cfg(desktop)]
#[tauri::command]
pub async fn check_app_update(app: AppHandle, state: State<'_, AppState>) -> Result<AppUpdateInfo> {
    let update = app_update::check(&app).await?;
    let running = app.package_info().version.to_string();
    let info = app_update::describe(update.as_ref(), &running);

    let mut pending = state
        .pending_update
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *pending = update;

    Ok(info)
}

/// Download, verify and install the manager release `check_app_update` found.
///
/// Windows never reports back: the NSIS installer takes over and the process
/// exits, relaunching the app once the new version is in place. macOS replaces
/// the bundle and relaunches from the backend.
#[cfg(desktop)]
#[tauri::command]
pub async fn install_app_update(app: AppHandle, state: State<'_, AppState>) -> Result<()> {
    let update = {
        let mut pending = state
            .pending_update
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        pending.take()
    };

    let Some(update) = update else {
        return Err(Error::msg("no manager update has been checked"));
    };

    let bytes = app_update::download(&app, &update).await?;
    app_update::install(&app, &update, bytes)
}

#[tauri::command]
pub async fn read_logs(state: State<'_, AppState>, limit: i64) -> Result<String> {
    let limit = if limit <= 0 || limit > 1000 {
        200usize
    } else {
        limit as usize
    };

    match run_blocking(move || platform::read_logs(limit)).await {
        Ok(text) => Ok(text),
        // The refresh button is the first place a root-owned log shows up, so
        // name the cause and repair it instead of handing the user EACCES.
        Err(err) if err.is_permission_denied() => {
            platform::prepare(state.inner()).await?;
            run_blocking(move || platform::read_logs(limit))
                .await
                .map_err(|_| Error::msg(LOG_PERMISSION_HINT))
        }
        Err(err) => Err(err),
    }
}

#[cfg(target_os = "macos")]
const LOG_PERMISSION_HINT: &str =
    "无法读取日志：日志文件权限不正确。请在系统弹窗中允许授权，或点击「安装」修复后重试。";
#[cfg(windows)]
const LOG_PERMISSION_HINT: &str = "无法读取日志：服务正在占用该日志文件，请稍后重试。";
#[cfg(target_os = "android")]
const LOG_PERMISSION_HINT: &str =
    "无法读取日志：应用无法访问自己的日志目录，请尝试重启应用。";

#[tauri::command]
pub async fn clear_logs(app: AppHandle, state: State<'_, AppState>) -> Result<Status> {
    platform::clear_log(state.inner()).await?;
    current_status(&app, state.inner()).await
}

#[tauri::command]
pub async fn get_settings(app: AppHandle) -> Result<Settings> {
    Ok(settings::load(&app))
}

#[tauri::command]
pub async fn save_settings(app: AppHandle, settings: Settings) -> Result<Settings> {
    settings::store(&app, &settings)
}

/// Leave for good. The window's own close button only hides it, so this is the
/// real exit — the same one the tray menu offers.
///
/// The managed service is deliberately left alone: it is what keeps the core
/// running while the manager is not, so quitting the UI must not stop it.
#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}

/// What `VpnService` has to be asked for on Android: the addresses, routes and
/// DNS the running instances expect.
///
/// The shell calls this after starting the core, hands the result to the
/// `vpnservice` plugin, and passes the descriptor it gets back to
/// [`android_attach_tun_fd`].
#[cfg(target_os = "android")]
#[tauri::command]
pub async fn android_tun_config() -> Result<TunPlan> {
    let plan = crate::mobile_core::tun_config().await;
    Ok(TunPlan::from(plan))
}

/// Attach the established TUN device to the running instances.
#[cfg(target_os = "android")]
#[tauri::command]
pub async fn android_attach_tun_fd(fd: i32) -> Result<TunPlan> {
    let plan = crate::mobile_core::attach_tun_fd(fd)
        .await
        .map_err(|err| Error::msg(format!("{err:#}")))?;
    Ok(TunPlan::from(plan))
}

/// The TUN plan in the shape the frontend and the plugin both want.
///
/// `addresses` carries the CIDR forms the instances claimed; the plugin's
/// `StartVpnArgs` only accepts a single address, so the first one is separately
/// exposed as `ipv4Addr` rather than making the shell slice strings.
#[cfg(target_os = "android")]
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TunPlan {
    pub addresses: Vec<String>,
    pub ipv4_addr: Option<String>,
    pub routes: Vec<String>,
    pub dns: Option<String>,
    pub instance_ids: Vec<String>,
    pub rejected: Vec<String>,
}

#[cfg(target_os = "android")]
impl From<easytier::instance::factory::MobileTunConfig> for TunPlan {
    fn from(config: easytier::instance::factory::MobileTunConfig) -> Self {
        Self {
            ipv4_addr: config.addresses.first().cloned(),
            addresses: config.addresses,
            routes: config.routes,
            dns: config.dns,
            instance_ids: config
                .instance_ids
                .into_iter()
                .map(|id| id.to_string())
                .collect(),
            rejected: config
                .rejected
                .into_iter()
                .map(|rejected| rejected.error)
                .collect(),
        }
    }
}
