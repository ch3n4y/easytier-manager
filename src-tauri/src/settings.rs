//! User-level settings, kept apart from the managed EasyTier configuration.
//!
//! The only setting so far is the GitHub download accelerator: release assets
//! are large and are frequently slow or unreachable on some networks, so the
//! user can point the app at a mirror.

use std::fs;
use std::path::PathBuf;
use std::sync::RwLock;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::error::{Error, Result};

/// Accelerator used when the user has not chosen one.
pub const DEFAULT_GITHUB_PROXY: &str = "https://gh-proxy.com/";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// Prefix prepended to GitHub URLs, e.g. `https://gh-proxy.com/`.
    pub github_proxy: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            github_proxy: DEFAULT_GITHUB_PROXY.to_string(),
        }
    }
}

/// Mirror of the on-disk setting, so the release helpers can read it without an
/// `AppHandle`. Empty means "not loaded yet"; the default is substituted.
static GITHUB_PROXY: RwLock<String> = RwLock::new(String::new());

/// The accelerator to use for the next request.
pub fn github_proxy() -> String {
    let configured = GITHUB_PROXY
        .read()
        .map(|value| value.clone())
        .unwrap_or_default();
    let trimmed = configured.trim();
    if trimmed.is_empty() {
        DEFAULT_GITHUB_PROXY.to_string()
    } else {
        trimmed.to_string()
    }
}

fn remember(settings: &Settings) {
    if let Ok(mut guard) = GITHUB_PROXY.write() {
        *guard = settings.github_proxy.trim().to_string();
    }
}

fn settings_path(app: &AppHandle) -> Result<PathBuf> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|err| Error::msg(err.to_string()))?;
    Ok(dir.join("settings.json"))
}

/// Read the settings from disk, falling back to the defaults on a first run or
/// an unreadable file. Always refreshes the in-memory copy.
pub fn load(app: &AppHandle) -> Settings {
    let settings = settings_path(app)
        .ok()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|data| serde_json::from_str::<Settings>(&data).ok())
        .unwrap_or_default();
    remember(&settings);
    settings
}

/// Persist the settings and refresh the in-memory copy.
pub fn store(app: &AppHandle, settings: &Settings) -> Result<Settings> {
    let path = settings_path(app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, serde_json::to_vec_pretty(settings)?)?;
    remember(settings);
    Ok(settings.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One test rather than two: the mirror of the setting is process-wide, so
    /// separate tests would race each other.
    #[test]
    fn resolves_the_configured_accelerator_with_a_default() {
        remember(&Settings {
            github_proxy: "   ".to_string(),
        });
        assert_eq!(github_proxy(), DEFAULT_GITHUB_PROXY);

        remember(&Settings {
            github_proxy: "  https://ghfast.top/  ".to_string(),
        });
        assert_eq!(github_proxy(), "https://ghfast.top/");
    }
}
