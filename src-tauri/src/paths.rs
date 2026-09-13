use std::path::{Path, PathBuf};

use crate::util::{shell_quote, shell_quote_path};

pub const SERVICE_LABEL: &str = "com.ch3n4y.easytier-desktop";
pub const INSTALL_ROOT: &str = "/Library/Application Support/EasyTier Desktop";
pub const LOG_ROOT: &str = "/Library/Logs/EasyTier Desktop";
pub const HELPER_LABEL: &str = "com.ch3n4y.easytier-desktop.helper";
pub const HELPER_PLIST: &str = "/Library/LaunchDaemons/com.ch3n4y.easytier-desktop.helper.plist";
pub const HELPER_SOCKET: &str = "/var/run/easytier-desktop-helper.sock";
pub const RELEASE_API_URL: &str = "https://api.github.com/repos/EasyTier/EasyTier/releases/latest";

pub const REQUIRED_BINARIES: [&str; 4] = [
    "easytier-core",
    "easytier-cli",
    "easytier-web",
    "easytier-web-embed",
];

pub const GITHUB_PROXY_PREFIXES: [&str; 3] = ["", "https://gh-proxy.com/", "https://ghproxy.net/"];

pub fn bin_dir() -> PathBuf {
    Path::new(INSTALL_ROOT).join("bin")
}

pub fn config_dir() -> PathBuf {
    Path::new(INSTALL_ROOT).join("config")
}

pub fn state_dir() -> PathBuf {
    Path::new(INSTALL_ROOT).join("state")
}

pub fn core_path() -> PathBuf {
    bin_dir().join("easytier-core")
}

pub fn service_script_path() -> PathBuf {
    bin_dir().join("easytier-service")
}

pub fn default_conf_path() -> PathBuf {
    config_dir().join("default.conf")
}

pub fn service_env_path() -> PathBuf {
    config_dir().join("service.env")
}

pub fn log_path() -> PathBuf {
    Path::new(LOG_ROOT).join("easytier.log")
}

pub fn pid_path() -> PathBuf {
    state_dir().join("easytier.pid")
}

// Templates use `__TOKEN__` placeholders rather than `format!` so the embedded
// shell and XML braces stay readable.

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

pub fn service_wrapper() -> String {
    SERVICE_WRAPPER
        .replace("__SERVICE_ENV__", &shell_quote_path(&service_env_path()))
        .replace("__DEFAULT_CONF__", &shell_quote_path(&default_conf_path()))
        .replace("__STATE_DIR__", &shell_quote_path(&state_dir()))
        .replace("__LOG__", &shell_quote_path(&log_path()))
        .replace("__PID__", &shell_quote_path(&pid_path()))
        .replace("__CORE__", &shell_quote_path(&core_path()))
}

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

pub fn helper_plist_content(exe: &str, uid: &str) -> String {
    HELPER_PLIST_TEMPLATE
        .replace("__LABEL__", HELPER_LABEL)
        .replace("__EXE__", exe)
        .replace("__UID__", uid)
        .replace("__LOG_ROOT__", LOG_ROOT)
}

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

#[cfg(test)]
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
