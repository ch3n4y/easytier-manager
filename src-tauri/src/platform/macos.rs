//! macOS privileged operations through the root LaunchDaemon helper, with a
//! one-shot `osascript` authorization fallback.

use std::fs;
use std::path::Path;

use tempfile::TempDir;

use crate::app::{
    admin_snapshot, one_shot_only, run_blocking, set_admin, set_one_shot_only, AppState,
};
use crate::error::Result;
use crate::helper::{
    ensure_helper, helper_is_ready, is_authorization_cancel, run_admin_script, run_admin_service,
    run_helper_script, run_helper_service,
};
use crate::paths::{
    config_dir, install_script, log_path, service_env_path, update_script, LOG_ROOT,
};
use crate::service::ServiceAction;
use crate::util::{shell_quote, shell_quote_path, tail_file};

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

async fn run_privileged(state: &AppState, script: &str) -> Result<()> {
    run_privileged_op(state, PrivilegedOp::Script(script.to_string())).await
}

async fn run_privileged_service(state: &AppState, action: ServiceAction) -> Result<()> {
    run_privileged_op(state, PrivilegedOp::Service(action)).await
}

pub async fn startup(state: &AppState) {
    let outcome = tokio::task::spawn_blocking(ensure_helper).await;
    match outcome {
        Ok(Ok(())) => set_admin(state, true, String::new()),
        Ok(Err(err)) => set_admin(state, false, err.to_string()),
        Err(err) => set_admin(state, false, err.to_string()),
    }
}

pub async fn prepare(state: &AppState) -> Result<()> {
    warm_helper(state).await
}

pub async fn refresh_admin(state: &AppState) {
    // Readiness is live state: re-probe on every status poll so a helper that
    // finished starting after a fallback automatically clears the stale
    // "待授权" UI. `admin` still carries the last fallback error.
    if run_blocking(|| Ok(helper_is_ready())).await.unwrap_or(false) {
        set_admin(state, true, String::new());
    } else {
        let error = admin_snapshot(state).1;
        set_admin(state, false, error);
    }
}

pub async fn install_privileged(state: &AppState, stage: &Path) -> Result<()> {
    let script = install_script(stage);
    run_privileged(state, &script).await
}

pub async fn update_privileged(state: &AppState, stage: &Path) -> Result<()> {
    let script = update_script(stage);
    run_privileged(state, &script).await
}

pub async fn apply_service(state: &AppState, action: ServiceAction) -> Result<()> {
    run_privileged_service(state, action).await
}

pub async fn write_config_server(state: &AppState, server: &str) -> Result<()> {
    let stage = TempDir::new()?;
    let staged_env = stage.path().join("service.env");
    fs::write(
        &staged_env,
        format!("EASYTIER_CONFIG_SERVER={}\n", shell_quote(server)),
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

    run_privileged(state, &script).await
}

pub async fn clear_log(state: &AppState) -> Result<()> {
    let script = [
        String::from("set -eu"),
        format!("mkdir -p {}", shell_quote(LOG_ROOT)),
        format!(": > {}", shell_quote_path(&log_path())),
    ]
    .join("\n");

    run_privileged(state, &script).await
}

pub fn read_logs(limit: usize) -> Result<String> {
    tail_file(&log_path(), limit)
}
