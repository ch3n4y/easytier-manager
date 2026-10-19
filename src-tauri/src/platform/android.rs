//! Android's privileged surface.
//!
//! There is no elevation to ask for: the app is the only owner of the core — it
//! *is* the core (see [`crate::mobile_core`]) — so every operation the desktop
//! builds route to launchd or the Service Control Manager runs in this process.
//! The one thing that genuinely needs a user decision, VPN consent, is asked for
//! by the shell through the `vpnservice` plugin, because only the frontend can
//! put a dialog on screen.

use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;

use crate::app::{run_blocking, set_admin, AppState};
use crate::config::read_config_files;
use crate::error::{Error, Result};
use crate::mobile_core;
use crate::paths::{
    config_dir, default_conf_path, default_config, log_path, log_root, service_env_path, state_dir,
};
use crate::service::ServiceAction;
use crate::util::tail_file;

/// No authorization exists to establish. The UI still reads this to decide
/// whether to show the "needs administrator rights" copy.
pub async fn startup(state: &AppState) {
    set_admin(state, true, String::new());
}

pub async fn refresh_admin(state: &AppState) {
    set_admin(state, true, String::new());
}

/// Nothing to prepare: the core is already in this process.
pub async fn prepare(_state: &AppState) -> Result<()> {
    Ok(())
}

/// First-run layout: the configuration the core reads and the log the panel
/// tails.
///
/// Synchronous on purpose — it runs from Tauri's `setup` hook, which cannot
/// await, and it is a handful of file operations.
pub fn initialize() -> Result<()> {
    for dir in [config_dir(), state_dir(), log_root()] {
        fs::create_dir_all(dir)?;
    }
    if !default_conf_path().exists() {
        fs::write(default_conf_path(), default_config())?;
    }
    // The shell's `parse_config_server` understands the quoted empty form, and
    // "no config server" is what selects local-config mode.
    if !service_env_path().exists() {
        fs::write(service_env_path(), "EASYTIER_CONFIG_SERVER=\"\"\n")?;
    }
    Ok(())
}

/// Send every `tracing` event into the file the log panel reads.
///
/// The core logs through `tracing` and does not install a subscriber of its own
/// (the CLI front-end does that), so this is the only thing deciding where its
/// output lands. Without it the panel would show nothing but the manager's own
/// lifecycle lines.
///
/// Timestamps are left out: `tracing-subscriber` only formats real times with an
/// extra `time`/`chrono` feature, and the wall clock a user needs when filing a
/// bug is in logcat anyway, next to the same lines.
pub fn init_logging() {
    let path = log_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let Ok(file) = File::options().create(true).append(true).open(&path) else {
        return;
    };
    let sink = Arc::new(Mutex::new(file));

    let writer = {
        let sink = sink.clone();
        move || SharedFile(sink.clone())
    };
    let layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .without_time()
        .with_writer(writer);

    // Another subscriber may already own the global slot; losing that race must
    // not take startup down, the panel just stays quiet.
    let _ = tracing_subscriber::registry().with(layer).try_init();
}

/// A writer handle the `fmt` layer can create one of per event.
struct SharedFile(Arc<Mutex<File>>);

impl Write for SharedFile {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut file = self
            .0
            .lock()
            .map_err(|_| std::io::Error::other("log file lock poisoned"))?;
        file.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let mut file = self
            .0
            .lock()
            .map_err(|_| std::io::Error::other("log file lock poisoned"))?;
        file.flush()
    }
}

/// The core ships inside the APK, so there is nothing to copy into place. Both
/// hooks exist because the shared lifecycle commands call them.
pub async fn install_privileged(_state: &AppState, _stage: &Path) -> Result<()> {
    Ok(())
}

pub async fn update_privileged(_state: &AppState, _stage: &Path) -> Result<()> {
    Ok(())
}

/// Start, stop or restart the in-process core.
///
/// "Install" is the first run of the same thing: the layout is created at
/// startup, so installing means starting.
pub async fn apply_service(_state: &AppState, action: ServiceAction) -> Result<()> {
    match action {
        ServiceAction::Stop => {
            mobile_core::stop().await.map_err(as_error)?;
            log_line("core stopped");
        }
        ServiceAction::Restart => {
            mobile_core::stop().await.map_err(as_error)?;
            start_core().await?;
        }
        ServiceAction::Install | ServiceAction::Start => start_core().await?,
    }
    Ok(())
}

/// Launch the core with whatever the config files currently say.
async fn start_core() -> Result<()> {
    let config = read_config_files();
    let server = config.config_server;
    let count = mobile_core::start(&server, &default_conf_path(), &state_dir())
        .await
        .map_err(as_error)?;

    if server.is_empty() {
        log_line(&format!(
            "core started from default.conf ({count} instance)"
        ));
    } else {
        log_line(&format!("core started against {server} ({count} instance)"));
    }
    Ok(())
}

/// The console address lives in `service.env`, which the core reads when it
/// starts — so a restart is what applies a change.
pub async fn write_config_server(_state: &AppState, server: &str) -> Result<()> {
    let server = server.to_string();
    // The closure runs on a blocking thread and needs an owned value, so it
    // gets its own copy; the log line below still reads the original.
    let for_write = server.clone();
    run_blocking(move || {
        fs::create_dir_all(config_dir())?;
        fs::write(
            service_env_path(),
            format!("EASYTIER_CONFIG_SERVER={for_write}\n"),
        )?;
        Ok(())
    })
    .await?;
    log_line(&format!("config server set to {server}"));
    Ok(())
}

pub async fn clear_log(_state: &AppState) -> Result<()> {
    run_blocking(|| {
        let path = log_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        // Truncate rather than delete: the tracing writer holds the handle.
        fs::write(&path, "")?;
        Ok(())
    })
    .await
}

pub fn read_logs(limit: usize) -> Result<String> {
    tail_file(&log_path(), limit)
}

/// Append one lifecycle line to the log the panel tails.
///
/// Epoch seconds rather than a formatted date: the file's purpose is ordering,
/// and the device clock is what the surrounding core lines are stamped with in
/// logcat.
pub fn log_line(message: &str) {
    let path = log_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let stamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);
    if let Ok(mut file) = File::options().create(true).append(true).open(&path) {
        let _ = writeln!(file, "[{stamp}] {message}");
    }
}

/// `anyhow` reaches the rest of the app as the crate's own error, which is what
/// the command layer serialises.
fn as_error(err: anyhow::Error) -> Error {
    Error::msg(format!("{err:#}"))
}
