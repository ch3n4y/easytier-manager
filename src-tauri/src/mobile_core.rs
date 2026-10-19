//! The EasyTier core, linked into the app instead of installed as a service.
//!
//! Android has no launchd/SCM equivalent, forbids executing a downloaded binary
//! out of app storage, and publishes no Android build of `easytier-core` — so
//! the desktop model (install a binary, register a service, poll it) simply does
//! not translate. What does translate is the official GUI's model, which this
//! module follows: run the core in-process through the `easytier` crate and let
//! the `vpnservice` plugin own the one thing a process cannot do for itself.
//!
//! That one thing is the TUN device. A non-root app only gets one from
//! `VpnService`, as a file descriptor it may keep, and `attach_mobile_tun_fd`
//! takes that descriptor and wires it into whichever running instances asked for
//! a TUN. Establishment needs user consent, which only the frontend can ask for,
//! so the sequence is: start instances here → read the TUN plan → let the shell
//! call the plugin → hand the fd back to [`attach_tun_fd`].
//!
//! The core's own tracing still goes to stdout, which on Android is logcat
//! (`adb logcat -s RustStdoutStderr`); the log panel reads the manager's own
//! file, written by [`crate::platform`].

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use anyhow::{anyhow, Context as _, Result};
use easytier::common::MachineIdOptions;
use easytier::instance::factory::{
    attach_mobile_tun_fd, get_mobile_tun_config, native_instance_manager, MobileTunConfig,
    NativeInstanceManager,
};
use easytier::web_client::{self, WebClient};
use easytier_core::config::toml::TomlConfig;
use easytier_core::instance::manager::ConfigFileControl;
use std::sync::Arc;

/// Whether the managed core has been started, as opposed to merely idle.
static STARTED: AtomicBool = AtomicBool::new(false);
/// How many network instances the core currently holds.
static INSTANCES: AtomicUsize = AtomicUsize::new(0);
/// Set once a TUN descriptor has been attached, so the shell can tell "core up
/// but the tunnel is not carrying traffic yet" from a working connection.
static TUN_ATTACHED: AtomicBool = AtomicBool::new(false);

struct Core {
    manager: Arc<NativeInstanceManager>,
    /// Alive only in config-server mode. Dropping it retires the instances the
    /// client created, which is why it is kept rather than discarded.
    web_client: Option<WebClient>,
}

/// The process-wide core. A single slot, because the app manages exactly one
/// tunnel at a time — same as the desktop service does.
static CORE: tokio::sync::Mutex<Option<Core>> = tokio::sync::Mutex::const_new(None);

/// Start the core the way the saved configuration describes.
///
/// A configured config server is the primary mode: the web client fetches the
/// network list and keeps it in step, exactly like `easytier-core -w <url>`.
/// Without one the bundled `default.conf` is loaded once, like `-c`.
pub async fn start(config_server: &str, config_file: &Path, state_dir: &Path) -> Result<usize> {
    let mut slot = CORE.lock().await;
    if slot.is_some() {
        return Ok(INSTANCES.load(Ordering::SeqCst));
    }

    let manager = Arc::new(native_instance_manager());
    let server = config_server.trim();

    let web_client = if server.is_empty() {
        let text = std::fs::read_to_string(config_file)
            .with_context(|| format!("读取配置失败: {}", config_file.display()))?;
        let config = TomlConfig::new_from_str(&text).context("解析配置失败")?;
        manager
            .run_network_instance(config, ConfigFileControl::STATIC_CONFIG)
            .context("启动网络实例失败")?;
        None
    } else {
        let options = MachineIdOptions {
            explicit_machine_id: None,
            state_dir: Some(state_dir.to_path_buf()),
        };
        Some(
            web_client::run_web_client(server, options, None, false, manager.clone(), None)
                .await
                .context("连接配置服务器失败")?,
        )
    };

    *slot = Some(Core { manager, web_client });
    STARTED.store(true, Ordering::SeqCst);
    count_instances(&slot);
    Ok(INSTANCES.load(Ordering::SeqCst))
}

/// Stop every instance and drop the config-server client.
pub async fn stop() -> Result<()> {
    let mut slot = CORE.lock().await;
    let Some(core) = slot.take() else {
        return Ok(());
    };

    // The config-server client owns what it created; the local-config mode has
    // no such owner, so the manager is asked either way.
    drop(core.web_client);
    core.manager
        .retain_network_instances(&[])
        .await
        .context("停止网络实例失败")?;

    STARTED.store(false, Ordering::SeqCst);
    TUN_ATTACHED.store(false, Ordering::SeqCst);
    INSTANCES.store(0, Ordering::SeqCst);
    Ok(())
}

/// What the shell has to ask `VpnService` for: addresses, routes and DNS.
pub async fn tun_config() -> MobileTunConfig {
    let slot = CORE.lock().await;
    let Some(core) = slot.as_ref() else {
        return MobileTunConfig::default();
    };
    let ids = core.manager.instance_ids();
    get_mobile_tun_config(core.manager.as_ref(), &ids).await
}

/// Hand the established TUN device to the running instances.
pub async fn attach_tun_fd(fd: i32) -> Result<MobileTunConfig> {
    let slot = CORE.lock().await;
    let core = slot.as_ref().ok_or_else(|| anyhow!("核心尚未启动"))?;
    let ids = core.manager.instance_ids();
    let config = attach_mobile_tun_fd(core.manager.as_ref(), fd, Some(ids.as_slice()))
        .await
        .context("挂接 TUN 设备失败")?;
    TUN_ATTACHED.store(true, Ordering::SeqCst);
    Ok(config)
}

/// Whether the core has been started.
pub fn is_started() -> bool {
    STARTED.load(Ordering::SeqCst)
}

/// Whether a tunnel is actually carrying traffic.
pub fn is_running() -> bool {
    TUN_ATTACHED.load(Ordering::SeqCst)
}

/// Number of live network instances, refreshed on demand.
pub fn instance_count() -> usize {
    if let Ok(slot) = CORE.try_lock() {
        count_instances(&slot);
    }
    INSTANCES.load(Ordering::SeqCst)
}

/// The core's version. It is linked in, so this is a compile-time constant
/// rather than something to ask a binary.
pub fn version() -> &'static str {
    easytier::VERSION
}

/// Record the instance count; takes the guard so the caller cannot race a
/// concurrent start or stop.
fn count_instances(slot: &Option<Core>) {
    let count = slot
        .as_ref()
        .map(|core| core.manager.instance_ids().len())
        .unwrap_or(0);
    INSTANCES.store(count, Ordering::SeqCst);
}
