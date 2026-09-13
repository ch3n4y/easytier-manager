use std::fs;
use std::io::Write;
use std::net::Shutdown;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::process::Command;
use std::sync::{mpsc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tempfile::TempDir;

use crate::error::{Error, Result};
use crate::launchd::{apply_service_action, ServiceAction};
use crate::paths::{helper_plist_content, HELPER_LABEL, HELPER_PLIST, HELPER_SOCKET, LOG_ROOT};
use crate::util::{apple_script_quote, shell_quote};

const CONNECT_TIMEOUT: Duration = Duration::from_millis(700);
const SOCKET_TIMEOUT: Duration = Duration::from_secs(30);
const READY_TIMEOUT: Duration = Duration::from_secs(2);
const HELPER_READY_TIMEOUT: Duration = Duration::from_secs(5);
const HELPER_PROTOCOL_VERSION: u32 = 2;
static HELPER_SETUP_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum HelperRequest {
    Script { script: String },
    Service { action: ServiceAction },
    Version,
}

#[derive(Debug, Serialize, Deserialize)]
struct HelperResponse {
    ok: bool,
    output: String,
    error: String,
    #[serde(default)]
    protocol_version: u32,
    /// The helper binary's own app version. The daemon outlives app upgrades,
    /// so the GUI compares this to its own version and re-installs the helper
    /// whenever they differ, ensuring helper-side changes actually take effect.
    #[serde(default)]
    app_version: String,
}

fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// True when this process was launched as the privileged helper rather than as
/// the GUI. The installed LaunchDaemon runs `<app binary> --helper <uid>`.
pub fn is_helper_invocation() -> bool {
    std::env::args().nth(1).as_deref() == Some("--helper")
}

/// Serve privileged shell requests over a uid-restricted unix socket. Never
/// returns.
pub fn run_helper() -> ! {
    let allowed_uid = std::env::var("EASYTIER_MANAGER_UID")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| std::env::args().nth(2));

    let _ = fs::remove_file(HELPER_SOCKET);
    let listener = match UnixListener::bind(HELPER_SOCKET) {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };

    // Only the invoking user may talk to the helper, even though it runs as root.
    if let Some(uid) = allowed_uid.and_then(|value| value.parse::<u32>().ok()) {
        let _ = std::os::unix::fs::chown(HELPER_SOCKET, Some(uid), None);
    }
    let _ = fs::set_permissions(HELPER_SOCKET, fs::Permissions::from_mode(0o600));

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                thread::spawn(move || handle_helper_conn(stream));
            }
            Err(_) => continue,
        }
    }

    std::process::exit(0)
}

fn handle_helper_conn(mut stream: UnixStream) {
    let reply = match serde_json::from_reader::<_, HelperRequest>(&stream) {
        Ok(request) => run_request(request),
        Err(err) => HelperResponse {
            ok: false,
            output: String::new(),
            error: err.to_string(),
            protocol_version: HELPER_PROTOCOL_VERSION,
            app_version: app_version(),
        },
    };

    let _ = serde_json::to_writer(&stream, &reply);
    let _ = stream.flush();
    let _ = stream.shutdown(Shutdown::Both);
}

fn run_request(request: HelperRequest) -> HelperResponse {
    match request {
        HelperRequest::Script { script } => run_script_request(script),
        HelperRequest::Service { action } => match apply_service_action(action) {
            Ok(()) => success_response(),
            Err(err) => error_response(err.to_string()),
        },
        HelperRequest::Version => success_response(),
    }
}

fn run_script_request(script: String) -> HelperResponse {
    if script.trim().is_empty() {
        return error_response("empty script");
    }

    match Command::new("/bin/sh").arg("-c").arg(&script).output() {
        Ok(output) => {
            let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
            combined.push_str(&String::from_utf8_lossy(&output.stderr));
            if output.status.success() {
                HelperResponse {
                    ok: true,
                    output: combined,
                    error: String::new(),
                    protocol_version: HELPER_PROTOCOL_VERSION,
                    app_version: app_version(),
                }
            } else {
                HelperResponse {
                    ok: false,
                    output: combined,
                    error: format!("exit status {}", output.status.code().unwrap_or(-1)),
                    protocol_version: HELPER_PROTOCOL_VERSION,
                    app_version: app_version(),
                }
            }
        }
        Err(err) => error_response(err.to_string()),
    }
}

fn success_response() -> HelperResponse {
    HelperResponse {
        ok: true,
        output: String::new(),
        error: String::new(),
        protocol_version: HELPER_PROTOCOL_VERSION,
        app_version: app_version(),
    }
}

fn error_response(error: impl Into<String>) -> HelperResponse {
    HelperResponse {
        ok: false,
        output: String::new(),
        error: error.into(),
        protocol_version: HELPER_PROTOCOL_VERSION,
        app_version: app_version(),
    }
}

/// Connect to the helper socket with a bounded dial, mirroring Go's
/// `net.DialTimeout`. `UnixStream::connect` takes no timeout of its own, so the
/// dial is raced against a channel receive.
fn connect_helper() -> Result<UnixStream> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let _ = sender.send(UnixStream::connect(HELPER_SOCKET));
    });

    match receiver.recv_timeout(CONNECT_TIMEOUT) {
        Ok(Ok(stream)) => Ok(stream),
        Ok(Err(err)) => Err(err.into()),
        Err(_) => Err(Error::msg("helper connection timed out")),
    }
}

fn send_request(request: &HelperRequest) -> Result<HelperResponse> {
    send_request_timeout(request, SOCKET_TIMEOUT)
}

fn send_request_timeout(request: &HelperRequest, read_timeout: Duration) -> Result<HelperResponse> {
    let stream = connect_helper()?;
    stream.set_read_timeout(Some(read_timeout))?;
    stream.set_write_timeout(Some(SOCKET_TIMEOUT))?;
    talk(&stream, request)
}

/// Write one request and read one reply on a connected helper stream.
///
/// The helper decodes the request with `serde_json::from_reader`, which only
/// returns once the read side reaches EOF. Half-close our write side so it sees
/// the end of the request and can reply — without this it blocks forever and
/// every call waits out the read timeout instead of being served.
fn talk(stream: &UnixStream, request: &HelperRequest) -> Result<HelperResponse> {
    {
        let mut writer = stream;
        serde_json::to_writer(&mut writer, request)?;
        writer.flush()?;
    }
    stream.shutdown(Shutdown::Write)?;

    Ok(serde_json::from_reader(stream)?)
}

fn require_success(response: HelperResponse) -> Result<()> {
    if response.ok {
        return Ok(());
    }
    if response.output.trim().is_empty() {
        return Err(Error::msg(response.error));
    }
    Err(Error::msg(format!(
        "{}: {}",
        response.error,
        response.output.trim()
    )))
}

/// Run a shell script through the privileged helper socket.
pub fn run_helper_script(script: &str) -> Result<()> {
    require_success(send_request(&HelperRequest::Script {
        script: script.to_string(),
    })?)
}

/// Run a typed EasyTier service operation through the privileged helper.
pub fn run_helper_service(action: ServiceAction) -> Result<()> {
    require_success(send_request(&HelperRequest::Service { action })?)
}

pub fn helper_is_ready() -> bool {
    // A short read timeout keeps a wedged daemon from stalling a status poll.
    send_request_timeout(&HelperRequest::Version, READY_TIMEOUT)
        .map(|response| helper_response_is_ready(&response))
        .unwrap_or(false)
}

/// A daemon only counts as ready when it speaks our protocol *and* runs our
/// build. The daemon outlives app upgrades, so ignoring the version would leave
/// helper-side fixes inert until the user manually removed the daemon.
fn helper_response_is_ready(response: &HelperResponse) -> bool {
    response.ok
        && response.protocol_version == HELPER_PROTOCOL_VERSION
        && response.app_version == app_version()
}

/// Install (or reuse) the root LaunchDaemon that backs [`run_helper_script`].
///
/// The daemon is a copy of this same binary re-invoked with `--helper`, so the
/// app ships no separate privileged executable.
pub fn ensure_helper() -> Result<()> {
    // App startup and a user action can both discover a missing helper. Keep
    // the authorization/install sequence single-flight so they cannot open
    // two password dialogs or restart the daemon underneath each other.
    let _setup_guard = HELPER_SETUP_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    if helper_is_ready() {
        return Ok(());
    }

    let exe = std::env::current_exe()?;
    let uid = unsafe { libc::getuid() }.to_string();

    let stage = TempDir::new()?;
    let plist_path = stage.path().join("helper.plist");
    fs::write(
        &plist_path,
        helper_plist_content(&exe.to_string_lossy(), &uid),
    )?;

    let script = [
        String::from("set -eu"),
        format!("mkdir -p {}", quote(LOG_ROOT)),
        // launchd writes helper.log here as root, and the GUI tails easytier.log
        // from the same directory as the invoking user — keep it traversable.
        format!("chmod 755 {}", quote(LOG_ROOT)),
        format!(
            "install -m 644 {} {}",
            quote(&plist_path.to_string_lossy()),
            quote(HELPER_PLIST)
        ),
        format!(
            "launchctl bootout system/{} 2>/dev/null || true",
            HELPER_LABEL
        ),
        format!("launchctl bootstrap system {}", quote(HELPER_PLIST)),
        format!("launchctl kickstart -k system/{}", HELPER_LABEL),
    ]
    .join("\n");

    run_admin_script(&script)?;

    let deadline = Instant::now() + HELPER_READY_TIMEOUT;
    while Instant::now() < deadline {
        if helper_is_ready() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(200));
    }

    Err(Error::msg("helper did not become ready"))
}

/// Execute a typed service operation through a one-shot administrator prompt.
/// This preserves the service-manager implementation even when the persistent
/// helper cannot be installed.
pub fn run_admin_service(action: ServiceAction) -> Result<()> {
    let exe = std::env::current_exe()?;
    let script = format!(
        "{} --service-action {}",
        quote(&exe.to_string_lossy()),
        action.as_arg()
    );
    run_admin_script(&script)
}

/// Fall back to a one-shot `osascript` authorization dialog when the daemon is
/// unavailable.
pub fn run_admin_script(script: &str) -> Result<()> {
    let apple_script = format!(
        "do shell script {} with administrator privileges",
        apple_script_quote(script)
    );

    let output = Command::new("osascript")
        .arg("-e")
        .arg(&apple_script)
        .output()?;
    if output.status.success() {
        return Ok(());
    }

    let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
    combined.push_str(&String::from_utf8_lossy(&output.stderr));
    Err(Error::msg(format!(
        "exit status {}: {}",
        output.status.code().unwrap_or(-1),
        combined.trim()
    )))
}

/// True when an AppleScript authorization prompt failed because the user
/// dismissed it, rather than because the privileged command itself failed.
/// `osascript` reports a dismissal as "User canceled. (-128)".
pub fn is_authorization_cancel(err: &Error) -> bool {
    let message = err.to_string();
    message.contains("User canceled") || message.contains("(-128)")
}

fn quote(value: &str) -> String {
    shell_quote(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    struct LegacyScriptRequest {
        script: String,
    }

    #[test]
    fn helper_plist_passes_uid_and_flag() {
        let plist = helper_plist_content(
            "/Applications/EasyTier Desktop.app/Contents/MacOS/easytier-desktop",
            "501",
        );
        for expected in [
            crate::paths::HELPER_LABEL,
            "--helper",
            "501",
            "EasyTier Desktop",
        ] {
            assert!(plist.contains(expected), "plist missing {expected}");
        }
    }

    #[test]
    fn detects_helper_invocation_shape() {
        // The daemon plist must launch us with the flag first.
        let plist = helper_plist_content("/tmp/app", "501");
        let flag = plist.find("--helper").unwrap();
        let uid = plist.find("<string>501</string>").unwrap();
        assert!(flag < uid, "--helper must precede the uid");
    }

    #[test]
    fn script_request_remains_compatible_with_legacy_helper() {
        let json = serde_json::to_string(&HelperRequest::Script {
            script: "true".to_string(),
        })
        .unwrap();
        let legacy: LegacyScriptRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(legacy.script, "true");
    }

    #[test]
    fn legacy_response_defaults_to_old_protocol() {
        let response: HelperResponse =
            serde_json::from_str(r#"{"ok":true,"output":"","error":""}"#).unwrap();
        assert_eq!(response.protocol_version, 0);
    }

    #[test]
    fn detects_authorization_cancellation() {
        let cancel = Error::msg("exit status 1: execution error: User canceled. (-128)");
        assert!(is_authorization_cancel(&cancel));

        let failure = Error::msg("exit status 1: launchctl: Operation not permitted");
        assert!(!is_authorization_cancel(&failure));
    }

    /// The daemon reads a request with `serde_json::from_reader`, which only
    /// returns at EOF. Guard against a client that sends the request but never
    /// half-closes, which would leave the helper blocked and the GUI hanging.
    #[test]
    fn request_is_half_closed_so_the_helper_can_reply() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("helper.sock");
        let listener = UnixListener::bind(&path).unwrap();

        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            // Mirror `handle_helper_conn`.
            let request: HelperRequest = serde_json::from_reader(&stream).unwrap();
            serde_json::to_writer(&stream, &run_request(request)).unwrap();
            let _ = stream.shutdown(Shutdown::Both);
        });

        let stream = UnixStream::connect(&path).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let reply = talk(&stream, &HelperRequest::Version).unwrap();

        assert!(reply.ok);
        assert_eq!(reply.protocol_version, HELPER_PROTOCOL_VERSION);
        assert!(helper_response_is_ready(&reply));
        server.join().unwrap();
    }

    #[test]
    fn readiness_requires_a_matching_helper_build() {
        assert!(helper_response_is_ready(&success_response()));

        // A daemon from before `app_version` existed must be reinstalled.
        let legacy: HelperResponse =
            serde_json::from_str(r#"{"ok":true,"output":"","error":"","protocol_version":2}"#)
                .unwrap();
        assert!(!helper_response_is_ready(&legacy));

        let stale = HelperResponse {
            ok: true,
            output: String::new(),
            error: String::new(),
            protocol_version: HELPER_PROTOCOL_VERSION,
            app_version: String::from("0.0.0"),
        };
        assert!(!helper_response_is_ready(&stale));
    }
}
