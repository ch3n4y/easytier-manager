//! Bridges to Android's package installer.
//!
//! An app cannot install a package by itself: Android requires the user to
//! confirm in a system dialog owned by the platform. So the only job here is to
//! build the intent, which needs a `Context` and a `FileProvider` URI — neither
//! of which Rust can reach without JNI. The Kotlin half lives in
//! `gen/android/app/src/main/java/com/ch3n4y/easytier_manager/ApkInstallerPlugin.kt`
//! and is registered as a Tauri mobile plugin below.

use serde::Serialize;
use tauri::plugin::{Builder, PluginHandle, TauriPlugin};
use tauri::{AppHandle, Manager, Runtime};

use crate::error::{Error, Result};

/// Holds the plugin handle so the command layer can call into Kotlin.
pub struct ApkInstaller<R: Runtime>(PluginHandle<R>);

#[derive(Clone, Serialize)]
struct InstallPayload<'a> {
    path: &'a str,
}

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("apkinstaller")
        .setup(|app, api| {
            let handle = api
                .register_android_plugin("com.ch3n4y.easytier_manager", "ApkInstallerPlugin")
                .map_err(|err| err.to_string())?;
            app.manage(ApkInstaller(handle));
            Ok(())
        })
        .build()
}

/// Hand `path` to the system installer.
///
/// Returns once the intent has been launched, which is not the same as the
/// package being installed — the user still decides in the dialog.
pub fn install<R: Runtime>(app: &AppHandle<R>, path: &str) -> Result<()> {
    let installer = app
        .try_state::<ApkInstaller<R>>()
        .ok_or_else(|| Error::msg("安装器插件未初始化"))?;
    installer
        .0
        .run_mobile_plugin::<serde_json::Value>("install", InstallPayload { path })
        .map_err(|err| Error::msg(format!("调起系统安装器失败：{err}")))?;
    Ok(())
}
