//! Self-update for the Android build.
//!
//! The desktop updater (`app_update`) verifies a minisign signature and lets
//! Tauri replace the bundle. Neither applies here: an APK is installed by the
//! system installer, and there is no signed archive to verify. So this module
//! reads the same GitHub release, picks the APK asset out of it, downloads that,
//! and hands the file to the installer via [`crate::apk_installer`].
//!
//! Nothing is silent: the user confirms in Android's own dialog, which is also
//! where the "install unknown apps" permission is granted the first time.

use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::AsyncWriteExt;

use crate::app::DownloadProgress;
use crate::error::{Error, Result};
use crate::paths::APP_RELEASE_API_URL;
use crate::release::{
    api_client, download_urls, fetch_release, normalize_version, GithubRelease, ReleaseAsset,
};

/// How often progress is forwarded, so a slow download still looks alive.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(200);

/// The name the release workflow gives the APK. Anything else in the release is
/// a desktop package and must not be offered as an Android update.
const APK_SUFFIX: &str = "-android.apk";

/// What the settings panel shows about a checked release. Same shape as the
/// desktop one so the frontend needs no branch.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub has_update: bool,
    pub notes: String,
    pub date: String,
}

/// The release and the APK asset inside it, kept together so installing uses
/// exactly the build the user was shown.
#[derive(Debug, Clone)]
pub struct Pending {
    pub release: GithubRelease,
    pub asset: ReleaseAsset,
}

pub fn describe(running: &str, pending: Option<&Pending>) -> AppUpdateInfo {
    let Some(pending) = pending else {
        return AppUpdateInfo {
            current_version: running.to_string(),
            latest_version: running.to_string(),
            has_update: false,
            notes: String::new(),
            date: String::new(),
        };
    };

    AppUpdateInfo {
        current_version: running.to_string(),
        latest_version: normalize_version(&pending.release.tag_name),
        has_update: true,
        notes: pending.release.body.clone().unwrap_or_default(),
        date: pending.release.published_at.clone().unwrap_or_default(),
    }
}

/// Resolve the latest release, if it carries an APK newer than this build.
pub async fn check(running: &str) -> Result<Option<Pending>> {
    let client = api_client()?;
    let mut last_error: Option<Error> = None;

    for url in crate::release::api_urls(APP_RELEASE_API_URL) {
        let release = match fetch_release(&client, &url).await {
            Ok(release) => release,
            Err(err) => {
                last_error = Some(err);
                continue;
            }
        };

        let Some(asset) = release
            .assets
            .iter()
            .find(|asset| asset.name.ends_with(APK_SUFFIX))
            .cloned()
        else {
            return Err(Error::msg(format!(
                "该版本没有安卓安装包（缺少 *{APK_SUFFIX}）"
            )));
        };

        let latest = normalize_version(&release.tag_name);
        if !is_newer(&latest, running) {
            return Ok(None);
        }
        return Ok(Some(Pending { release, asset }));
    }

    Err(last_error.unwrap_or_else(|| Error::msg("github release request failed")))
}

/// Download the APK into the app cache and return its path.
///
/// The cache directory is what `res/xml/file_paths.xml` exposes to the
/// FileProvider, so the installer can read the file without any storage
/// permission.
pub async fn download(app: &AppHandle, asset: &ReleaseAsset) -> Result<PathBuf> {
    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|err| Error::msg(format!("无法定位缓存目录：{err}")))?;
    std::fs::create_dir_all(&dir)?;
    let target = dir.join("manager-update.apk");

    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(600))
        .build()?;

    let mut last_error: Option<Error> = None;
    for url in download_urls(&asset.browser_download_url) {
        match fetch_to(&client, app, &url, &target).await {
            Ok(()) => return Ok(target),
            Err(err) => {
                last_error = Some(err);
                continue;
            }
        }
    }

    Err(last_error.unwrap_or_else(|| Error::msg("下载安卓安装包失败")))
}

async fn fetch_to(
    client: &reqwest::Client,
    app: &AppHandle,
    url: &str,
    target: &PathBuf,
) -> Result<()> {
    let response = client.get(url).send().await?;
    if !response.status().is_success() {
        return Err(Error::msg(format!("下载失败：{}", response.status())));
    }

    let total = response.content_length();
    let mut file = tokio::fs::File::create(target).await?;
    let mut received: u64 = 0;
    let mut since = std::time::Instant::now();
    let mut body = response;

    while let Some(chunk) = body.chunk().await? {
        file.write_all(&chunk).await?;
        received += chunk.len() as u64;
        if since.elapsed() >= PROGRESS_INTERVAL {
            since = std::time::Instant::now();
            let _ = app.emit(
                "download-progress",
                DownloadProgress { received, total },
            );
        }
    }

    file.flush().await?;
    let _ = app.emit("download-progress", DownloadProgress { received, total });
    Ok(())
}

/// Hand the downloaded APK to the system installer.
pub fn install(app: &AppHandle, path: &PathBuf) -> Result<()> {
    if !path.is_file() {
        return Err(Error::msg("安装包不存在，请重新下载"));
    }
    crate::apk_installer::install(app, &path.to_string_lossy())
}

/// True when `latest` is a higher dotted version than `current`.
fn is_newer(latest: &str, current: &str) -> bool {
    let parse = |text: &str| -> Vec<u64> {
        text.split(['.', '-'])
            .map(|part| part.parse::<u64>().unwrap_or(0))
            .collect()
    };

    let latest = parse(latest);
    let current = parse(current);
    let depth = latest.len().max(current.len());
    for index in 0..depth {
        let a = latest.get(index).copied().unwrap_or(0);
        let b = current.get(index).copied().unwrap_or(0);
        if a != b {
            return a > b;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::is_newer;

    #[test]
    fn compares_dotted_versions() {
        assert!(is_newer("0.5.0", "0.4.3"));
        assert!(is_newer("0.4.10", "0.4.9"));
        assert!(is_newer("1.0", "0.9.9"));
        assert!(!is_newer("0.4.3", "0.4.3"));
        assert!(!is_newer("0.4.2", "0.4.3"));
    }
}
