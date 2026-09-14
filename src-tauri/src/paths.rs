use std::path::PathBuf;

#[cfg(target_os = "macos")]
use std::path::Path;

#[cfg(target_os = "macos")]
use crate::util::{shell_quote, shell_quote_path};

pub const RELEASE_API_URL: &str = "https://api.github.com/repos/EasyTier/EasyTier/releases/latest";

pub const GITHUB_PROXY_PREFIXES: [&str; 3] = ["", "https://gh-proxy.com/", "https://ghproxy.net/"];

/// The managed EasyTier binaries. Windows ships `.exe` files; macOS does not.
#[cfg(target_os = "macos")]
pub const REQUIRED_BINARIES: [&str; 4] = [
    "easytier-core",
    "easytier-cli",
    "easytier-web",
    "easytier-web-embed",
];

#[cfg(windows)]
pub const REQUIRED_BINARIES: [&str; 4] = [
    "easytier-core.exe",
    "easytier-cli.exe",
    "easytier-web.exe",
    "easytier-web-embed.exe",
];

/// Non-executable payloads the core needs at runtime. Windows requires
/// `wintun.dll` to create the virtual network adapter; other platforms bundle
/// nothing extra.
#[cfg(windows)]
pub const REQUIRED_LIBRARIES: [&str; 1] = ["wintun.dll"];

#[cfg(target_os = "macos")]
pub const REQUIRED_LIBRARIES: [&str; 0] = [];

/// Runtime helpers the official Windows package ships next to the core. WinDivert
/// backs EasyTier's packet-capture features, so it is copied along with the
/// binaries — but only when the release actually contains it, so a release that
/// stops bundling these cannot break an install.
#[cfg(windows)]
pub const OPTIONAL_LIBRARIES: [&str; 2] = ["Packet.dll", "WinDivert64.sys"];

#[cfg(target_os = "macos")]
pub const OPTIONAL_LIBRARIES: [&str; 0] = [];

// ── Shared layout ─────────────────────────────────────────────────────────

/// Root the app manages. On macOS this is a fixed system path; on Windows it
/// lives under `%ProgramData%`.
#[cfg(target_os = "macos")]
pub fn install_root() -> PathBuf {
    PathBuf::from(INSTALL_ROOT)
}

#[cfg(windows)]
pub fn install_root() -> PathBuf {
    let base = std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
    base.join("EasyTier Manager")
}

/// Directory holding the service log. On macOS it is a dedicated `/Library/Logs`
/// tree; on Windows it is nested under the install root and fed to the core
/// through `--file-log-dir`.
#[cfg(target_os = "macos")]
pub fn log_root() -> PathBuf {
    PathBuf::from(LOG_ROOT)
}

#[cfg(windows)]
pub fn log_root() -> PathBuf {
    install_root().join("logs")
}

pub fn bin_dir() -> PathBuf {
    install_root().join("bin")
}

pub fn config_dir() -> PathBuf {
    install_root().join("config")
}

pub fn state_dir() -> PathBuf {
    install_root().join("state")
}

#[cfg(target_os = "macos")]
pub const CORE_BINARY: &str = "easytier-core";

#[cfg(windows)]
pub const CORE_BINARY: &str = "easytier-core.exe";

pub fn core_path() -> PathBuf {
    bin_dir().join(CORE_BINARY)
}

pub fn default_conf_path() -> PathBuf {
    config_dir().join("default.conf")
}

pub fn service_env_path() -> PathBuf {
    config_dir().join("service.env")
}

/// The managed service log. On Windows the core writes rolling files into
/// [`log_root`], so this is only the default/primary candidate name.
pub fn log_path() -> PathBuf {
    log_root().join("easytier.log")
}

pub fn default_config() -> String {
    String::from(
        r#"instance_name = "default"
dhcp = true
listeners = [
    "tcp://0.0.0.0:11010",
    "udp://0.0.0.0:11010",
    "wg://0.0.0.0:11011",
    "ws://0.0.0.0:11011/",
    "wss://0.0.0.0:11012/",
]
exit_nodes = []
rpc_portal = "0.0.0.0:0"

[[peer]]
uri = "tcp://public.easytier.top:11010"

[network_identity]
network_name = "default"
network_secret = "default"

[flags]
default_protocol = "udp"
dev_name = ""
enable_encryption = true
enable_ipv6 = true
mtu = 1380
latency_first = false
enable_exit_node = false
no_tun = false
use_smoltcp = false
foreign_network_whitelist = "*"
disable_p2p = false
p2p_only = false
relay_all_peer_rpc = false
disable_tcp_hole_punching = false
disable_udp_hole_punching = false
"#,
    )
}

// ── macOS ─────────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
pub const SERVICE_LABEL: &str = "com.ch3n4y.easytier-manager";
#[cfg(target_os = "macos")]
pub const INSTALL_ROOT: &str = "/Library/Application Support/EasyTier Manager";
#[cfg(target_os = "macos")]
pub const LOG_ROOT: &str = "/Library/Logs/EasyTier Manager";
#[cfg(target_os = "macos")]
pub const HELPER_LABEL: &str = "com.ch3n4y.easytier-manager.helper";
#[cfg(target_os = "macos")]
pub const HELPER_PLIST: &str =
    "/Library/LaunchDaemons/com.ch3n4y.easytier-manager.helper.plist";
#[cfg(target_os = "macos")]
pub const HELPER_SOCKET: &str = "/var/run/easytier-manager-helper.sock";

/// The LaunchDaemon plist `service-manager` writes for the managed service.
#[cfg(target_os = "macos")]
pub fn service_plist_path() -> PathBuf {
    Path::new("/Library/LaunchDaemons").join(format!("{SERVICE_LABEL}.plist"))
}

/// The shell wrapper the managed service runs.
#[cfg(target_os = "macos")]
pub fn service_script_path() -> PathBuf {
    bin_dir().join("easytier-service")
}

/// PID file written by whichever wrapper owns the running core: the macOS shell
/// script, or the Windows service host.
pub fn pid_path() -> PathBuf {
    state_dir().join("easytier.pid")
}

// Templates use `__TOKEN__` placeholders rather than `format!` so the embedded
// shell and XML braces stay readable.

#[cfg(target_os = "macos")]
const SERVICE_WRAPPER: &str = r#"#!/bin/sh
set -eu

service_env=__SERVICE_ENV__
config_file=__DEFAULT_CONF__
state_dir=__STATE_DIR__
log_file=__LOG__
pid_file=__PID__

if [ -r "$service_env" ]; then
  . "$service_env"
fi

mkdir -p "$state_dir"
export HOME="${EASYTIER_HOME:-$state_dir}"
export XDG_STATE_HOME="${XDG_STATE_HOME:-$state_dir}"
umask 022
echo "$$" > "$pid_file"
exec >> "$log_file" 2>&1

if [ -n "${EASYTIER_CONFIG_SERVER:-}" ]; then
  exec __CORE__ -w "$EASYTIER_CONFIG_SERVER"
fi

exec __CORE__ -c "$config_file"
"#;

#[cfg(target_os = "macos")]
pub fn service_wrapper() -> String {
    SERVICE_WRAPPER
        .replace("__SERVICE_ENV__", &shell_quote_path(&service_env_path()))
        .replace("__DEFAULT_CONF__", &shell_quote_path(&default_conf_path()))
        .replace("__STATE_DIR__", &shell_quote_path(&state_dir()))
        .replace("__LOG__", &shell_quote_path(&log_path()))
        .replace("__PID__", &shell_quote_path(&pid_path()))
        .replace("__CORE__", &shell_quote_path(&core_path()))
}

#[cfg(target_os = "macos")]
const HELPER_PLIST_TEMPLATE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>__LABEL__</string>
  <key>ProgramArguments</key>
  <array>
    <string>__EXE__</string>
    <string>--helper</string>
    <string>__UID__</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>__LOG_ROOT__/helper.log</string>
  <key>StandardErrorPath</key>
  <string>__LOG_ROOT__/helper.log</string>
</dict>
</plist>
"#;

#[cfg(target_os = "macos")]
pub fn helper_plist_content(exe: &str, uid: &str) -> String {
    HELPER_PLIST_TEMPLATE
        .replace("__LABEL__", HELPER_LABEL)
        .replace("__EXE__", exe)
        .replace("__UID__", uid)
        .replace("__LOG_ROOT__", LOG_ROOT)
}

#[cfg(target_os = "macos")]
const INSTALL_SCRIPT: &str = r#"set -eu
mkdir -p __BIN_DIR__ __CONFIG_DIR__ __STATE_DIR__ __LOG_ROOT__
# The GUI runs as the invoking user and tails these files directly, so both the
# directory (needs +x to traverse) and the log have to be readable. An older
# install may have left the directory at 0744, and `mkdir -p` will not correct
# an existing mode.
chmod 755 __LOG_ROOT__
install -m 755 __STAGE_CORE__ __CORE__
install -m 755 __STAGE_CLI__ __CLI__
install -m 755 __STAGE_WEB__ __WEB__
install -m 755 __STAGE_WEB_EMBED__ __WEB_EMBED__
install -m 755 __STAGE_SERVICE__ __SERVICE_SCRIPT__
if [ ! -f __DEFAULT_CONF__ ]; then install -m 644 __STAGE_DEFAULT_CONF__ __DEFAULT_CONF__; fi
if [ ! -f __SERVICE_ENV__ ]; then install -m 644 __STAGE_SERVICE_ENV__ __SERVICE_ENV__; fi
touch __LOG__
chmod 644 __LOG__"#;

#[cfg(target_os = "macos")]
pub fn install_script(stage: &Path) -> String {
    let bin = bin_dir();
    INSTALL_SCRIPT
        .replace("__BIN_DIR__", &shell_quote_path(&bin))
        .replace("__CONFIG_DIR__", &shell_quote_path(&config_dir()))
        .replace("__STATE_DIR__", &shell_quote_path(&state_dir()))
        .replace("__LOG_ROOT__", &shell_quote(LOG_ROOT))
        .replace("__CORE__", &shell_quote_path(&core_path()))
        .replace("__CLI__", &shell_quote_path(&bin.join("easytier-cli")))
        .replace("__WEB__", &shell_quote_path(&bin.join("easytier-web")))
        .replace(
            "__WEB_EMBED__",
            &shell_quote_path(&bin.join("easytier-web-embed")),
        )
        .replace(
            "__SERVICE_SCRIPT__",
            &shell_quote_path(&service_script_path()),
        )
        .replace("__DEFAULT_CONF__", &shell_quote_path(&default_conf_path()))
        .replace("__SERVICE_ENV__", &shell_quote_path(&service_env_path()))
        .replace("__LOG__", &shell_quote_path(&log_path()))
        .replace(
            "__STAGE_CORE__",
            &shell_quote_path(&stage.join("bin").join("easytier-core")),
        )
        .replace(
            "__STAGE_CLI__",
            &shell_quote_path(&stage.join("bin").join("easytier-cli")),
        )
        .replace(
            "__STAGE_WEB__",
            &shell_quote_path(&stage.join("bin").join("easytier-web")),
        )
        .replace(
            "__STAGE_WEB_EMBED__",
            &shell_quote_path(&stage.join("bin").join("easytier-web-embed")),
        )
        .replace(
            "__STAGE_SERVICE__",
            &shell_quote_path(&stage.join("easytier-service")),
        )
        .replace(
            "__STAGE_DEFAULT_CONF__",
            &shell_quote_path(&stage.join("default.conf")),
        )
        .replace(
            "__STAGE_SERVICE_ENV__",
            &shell_quote_path(&stage.join("service.env")),
        )
}

#[cfg(target_os = "macos")]
const UPDATE_SCRIPT: &str = r#"set -eu
backup=""
if [ -d __BIN_DIR__ ]; then backup=__INSTALL_ROOT__/bin.backup.$(date +%s); cp -pR __BIN_DIR__ "$backup"; fi
restore_backup() { if [ -n "${backup:-}" ] && [ -d "$backup" ]; then rm -rf __BIN_DIR__; mv "$backup" __BIN_DIR__; fi; }
trap 'code=$?; if [ $code -ne 0 ]; then restore_backup; fi; exit $code' EXIT
mkdir -p __BIN_DIR__
install -m 755 __STAGE_CORE__ __CORE__
install -m 755 __STAGE_CLI__ __CLI__
install -m 755 __STAGE_WEB__ __WEB__
install -m 755 __STAGE_WEB_EMBED__ __WEB_EMBED__
trap - EXIT"#;

#[cfg(target_os = "macos")]
pub fn update_script(stage: &Path) -> String {
    let bin = bin_dir();
    UPDATE_SCRIPT
        .replace("__BIN_DIR__", &shell_quote_path(&bin))
        .replace("__INSTALL_ROOT__", &shell_quote(INSTALL_ROOT))
        .replace("__CORE__", &shell_quote_path(&core_path()))
        .replace("__CLI__", &shell_quote_path(&bin.join("easytier-cli")))
        .replace("__WEB__", &shell_quote_path(&bin.join("easytier-web")))
        .replace(
            "__WEB_EMBED__",
            &shell_quote_path(&bin.join("easytier-web-embed")),
        )
        .replace(
            "__STAGE_CORE__",
            &shell_quote_path(&stage.join("bin").join("easytier-core")),
        )
        .replace(
            "__STAGE_CLI__",
            &shell_quote_path(&stage.join("bin").join("easytier-cli")),
        )
        .replace(
            "__STAGE_WEB__",
            &shell_quote_path(&stage.join("bin").join("easytier-web")),
        )
        .replace(
            "__STAGE_WEB_EMBED__",
            &shell_quote_path(&stage.join("bin").join("easytier-web-embed")),
        )
}

// ── Windows ───────────────────────────────────────────────────────────────

/// Name the service is registered under in the Service Control Manager.
#[cfg(windows)]
pub const WINDOWS_SERVICE_NAME: &str = "EasyTierManager";
/// User-facing service name shown by `services.msc`.
#[cfg(windows)]
pub const WINDOWS_SERVICE_DISPLAY_NAME: &str = "EasyTier Manager";

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn wrapper_owns_logging_and_pid_tracking() {
        let wrapper = service_wrapper();
        assert!(wrapper.contains("easytier.pid"));
        assert!(wrapper.contains("easytier.log"));
        assert!(wrapper.contains("exec >>"));
    }

    #[test]
    fn install_and_update_scripts_do_not_manage_launchd() {
        let stage = Path::new("/tmp/easytier stage");
        assert!(!install_script(stage).contains("launchctl"));
        assert!(!update_script(stage).contains("launchctl"));
    }
}
