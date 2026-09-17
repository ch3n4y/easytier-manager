//! Self-update for the manager application itself.
//!
//! The managed EasyTier core has its own release channel — see [`crate::release`].
//! This module covers the app bundle, which ships as macOS DMG and Windows
//! NSIS/MSI installers through GitHub Releases.
//!
//! Updates go through `tauri-plugin-updater`, which verifies a minisign
//! signature over the downloaded package against the public key in
//! `tauri.conf.json` before anything is installed. The private half of that pair
//! exists only in the release workflow, so a compromised mirror cannot talk the
//! app into installing something else.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Url};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::app::DownloadProgress;
use crate::error::{Error, Result};
use crate::paths::UPDATER_ENDPOINT;
use crate::release::{api_urls, download_urls};

/// Fail fast on an unreachable mirror so the next one gets a turn, the same way
/// the core download does.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// A mirror that accepts the connection and then stops sending data must not
/// wedge the check forever. There is deliberately no cap on the whole transfer:
/// the package is a few megabytes and a throttled one can legitimately take
/// minutes.
const READ_TIMEOUT: Duration = Duration::from_secs(30);
/// How often download progress is forwarded to the webview.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(200);
/// How long the macOS relaunch waits, so the command's reply reaches the webview
/// before the process goes away.
#[cfg(target_os = "macos")]
const RELAUNCH_DELAY: Duration = Duration::from_millis(800);

/// What the settings panel shows about a checked release.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub has_update: bool,
    /// The release notes, empty when there is nothing to install.
    pub notes: String,
    /// Publication date of the release, empty when the manifest omits it.
    pub date: String,
}

/// Describe a checked release for the settings panel. `running` is the version
/// of the build doing the asking, which is all there is to report when the
/// channel has nothing newer.
pub fn describe(update: Option<&Update>, running: &str) -> AppUpdateInfo {
    let Some(update) = update else {
        return AppUpdateInfo {
            current_version: running.to_string(),
            latest_version: running.to_string(),
            has_update: false,
            notes: String::new(),
            date: String::new(),
        };
    };

    AppUpdateInfo {
        current_version: update.current_version.clone(),
        latest_version: update.version.clone(),
        has_update: true,
        notes: update.body.clone().unwrap_or_default(),
        // Read out of the raw manifest rather than the typed field, which would
        // drag in a date crate just to turn it back into a string.
        date: update
            .raw_json
            .get("pub_date")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
    }
}

/// Resolve the latest manager release, if it is newer than the running build.
///
/// The [`Update`] is handed back rather than merely described so that installing
/// uses exactly the release the user was shown, instead of whatever the channel
/// holds by the time they click.
pub async fn check(app: &AppHandle) -> Result<Option<Update>> {
    let updater = app
        .updater_builder()
        .endpoints(endpoint_urls()?)?
        .configure_client(|builder| {
            builder
                .connect_timeout(CONNECT_TIMEOUT)
                .read_timeout(READ_TIMEOUT)
        })
        .build()?;

    Ok(updater.check().await?)
}

/// The release manifest, behind the user's accelerator first: the same mirror
/// chain the EasyTier core download uses, so a network that cannot reach GitHub
/// can still update the manager. The updater walks the list itself, moving on
/// whenever one endpoint fails.
fn endpoint_urls() -> Result<Vec<Url>> {
    let mut endpoints = Vec::new();

    for candidate in api_urls(UPDATER_ENDPOINT) {
        // A release build refuses any endpoint that is not https, so an
        // accelerator configured as plain http is dropped here rather than
        // failing the whole check. Direct GitHub access stays in the list, so
        // there is always a candidate left.
        if !candidate.starts_with("https://") {
            continue;
        }
        if let Ok(url) = Url::parse(&candidate) {
            endpoints.push(url);
        }
    }

    if endpoints.is_empty() {
        return Err(Error::msg("no usable update endpoint"));
    }
    Ok(endpoints)
}

/// Download the package for `update`, verifying its signature.
pub async fn download(app: &AppHandle, update: &Update) -> Result<Vec<u8>> {
    let mut last_error: Option<Error> = None;

    // The accelerators only rewrite GitHub URLs, so an update served from
    // anywhere else is fetched directly.
    for candidate in download_urls(update.download_url.as_str()) {
        let Ok(url) = Url::parse(&candidate) else {
            continue;
        };

        // `download` verifies the announced signature before handing the bytes
        // back, so a mirror that serves a truncated or tampered package fails
        // here — and the next one gets a turn.
        let mut attempt = update.clone();
        attempt.download_url = url;

        let reporter = ProgressReporter::new(app);
        let on_chunk = reporter.clone();
        let result = attempt
            .download(
                move |length, total| on_chunk.chunk(length, total),
                move || reporter.finish(),
            )
            .await;

        match result {
            Ok(bytes) => return Ok(bytes),
            Err(err) => last_error = Some(err.into()),
        }
    }

    Err(last_error.unwrap_or_else(|| Error::msg("update download failed")))
}

/// Install a downloaded package.
///
/// The two platforms part ways here: on Windows the installer is handed the
/// package and the process exits, while on macOS the bundle is replaced in place
/// and the app has to bring itself back up.
pub fn install(
    #[cfg_attr(windows, allow(unused_variables))] app: &AppHandle,
    update: &Update,
    bytes: Vec<u8>,
) -> Result<()> {
    update.install(bytes)?;

    #[cfg(target_os = "macos")]
    {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(RELAUNCH_DELAY).await;
            app.restart();
        });
    }

    Ok(())
}

/// Counts the bytes the updater streams at us and forwards them to the webview
/// at most every [`PROGRESS_INTERVAL`]. The plugin reports every network chunk,
/// which would otherwise flood the IPC channel.
#[derive(Clone)]
struct ProgressReporter {
    app: AppHandle,
    state: Arc<Mutex<ProgressState>>,
}

struct ProgressState {
    received: u64,
    total: Option<u64>,
    last: Instant,
}

impl ProgressReporter {
    fn new(app: &AppHandle) -> Self {
        Self {
            app: app.clone(),
            state: Arc::new(Mutex::new(ProgressState {
                received: 0,
                total: None,
                last: Instant::now(),
            })),
        }
    }

    /// Count a chunk, reporting it only once the interval has passed.
    fn chunk(&self, length: usize, total: Option<u64>) {
        let mut state = self.lock();
        state.received += length as u64;
        state.total = total.or(state.total);

        if state.last.elapsed() < PROGRESS_INTERVAL {
            return;
        }
        state.last = Instant::now();
        self.emit(state.received, state.total);
    }

    /// Report the final count, which the interval above would otherwise swallow.
    fn finish(&self) {
        let state = self.lock();
        let received = state.received;
        self.emit(received, Some(state.total.unwrap_or(received)));
    }

    fn lock(&self) -> MutexGuard<'_, ProgressState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn emit(&self, received: u64, total: Option<u64>) {
        let _ = self
            .app
            .emit("download-progress", DownloadProgress { received, total });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_https_endpoints_behind_the_accelerator() {
        let endpoints = endpoint_urls().unwrap();
        let urls: Vec<&str> = endpoints.iter().map(|url| url.as_str()).collect();

        // Direct access is always in the chain, so a check cannot be left with
        // nowhere to go when the accelerator is unusable.
        assert!(urls.contains(&UPDATER_ENDPOINT));
        assert!(urls.iter().all(|url| url.starts_with("https://")));
    }

    #[test]
    fn reports_the_running_version_when_there_is_nothing_to_install() {
        let info = describe(None, "0.2.1");

        assert!(!info.has_update);
        assert_eq!(info.current_version, "0.2.1");
        assert_eq!(info.latest_version, "0.2.1");
        assert!(info.notes.is_empty());
    }
}
